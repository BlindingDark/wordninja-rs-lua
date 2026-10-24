//! Lua bindings for the preset model and independent, extended vocabulary models.
use mlua::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

mod model;
use model::{LanguageModel, DEFAULT_MODEL};

const DEFAULT_USER_COST: f64 = 6.0;
const MAX_WORD_BYTES: usize = 128;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_RIME_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn valid_word(word: &str) -> bool {
    word.len() <= MAX_WORD_BYTES
        && word.bytes().any(|b| b.is_ascii_alphanumeric())
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'\'')
}

#[derive(Debug, Default)]
struct RimeWords {
    words: HashSet<String>,
    files: usize,
    entries: usize,
    duplicates: usize,
    skipped: usize,
}

impl RimeWords {
    fn insert(&mut self, value: &str) -> bool {
        let word = value.to_ascii_lowercase();
        if !valid_word(&word) {
            return false;
        }
        if !self.words.insert(word) {
            self.duplicates += 1;
        }
        true
    }
}

fn parse_rime_dictionary(text: &str, result: &mut RimeWords) -> Result<(), String> {
    let lines: Vec<_> = text.trim_start_matches('\u{feff}').lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim() == "---")
        .ok_or("missing Rime dictionary header (---)")?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim() == "...")
        .map(|(index, _)| index)
        .ok_or("missing Rime dictionary header terminator (...)")?;
    let header: serde_yaml::Value = serde_yaml::from_str(&lines[start + 1..end].join("\n"))
        .map_err(|error| format!("invalid dictionary YAML header: {error}"))?;
    if !header.is_mapping() {
        return Err("dictionary header must be a YAML mapping".into());
    }
    let columns: Vec<&str> = match &header["columns"] {
        serde_yaml::Value::Null => vec!["text", "code", "weight"],
        serde_yaml::Value::Sequence(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| "dictionary columns must be strings".to_string())
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("dictionary columns must be a YAML sequence".into()),
    };
    let text_column = columns
        .iter()
        .position(|&name| name == "text")
        .ok_or("dictionary columns must include text")?;
    let code_column = columns.iter().position(|&name| name == "code");
    for raw in &lines[end + 1..] {
        if raw.trim().is_empty() || raw.trim_start().starts_with('#') {
            continue;
        }
        result.entries += 1;
        let fields: Vec<_> = raw.split('\t').collect();
        let mut accepted = false;
        // Match the splitter's existing ASCII token boundaries; never join display punctuation.
        if let Some(text) = fields.get(text_column) {
            for token in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '\'')) {
                accepted |= result.insert(token);
            }
        }
        // Simple spelling aliases are also useful (String::from -> stringfrom).
        // Do not split space-delimited syllables or interpret a weight as a word.
        if let Some(code) = code_column.and_then(|index| fields.get(index)) {
            accepted |= result.insert(code.trim());
        }
        if !accepted {
            result.skipped += 1;
        }
    }
    result.files += 1;
    Ok(())
}

fn load_rime_directory(path: &Path) -> Result<RimeWords, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        // Non-recursive, regular files only: no directory/symlink traversal.
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".dict.yaml")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    let mut result = RimeWords::default();
    for file in files {
        if fs::metadata(&file)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_RIME_FILE_BYTES
        {
            return Err(format!(
                "{}: Rime dictionary exceeds 16 MiB",
                file.display()
            ));
        }
        let text =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        parse_rime_dictionary(&text, &mut result)
            .map_err(|error| format!("{}: {error}", file.display()))?;
    }
    Ok(result)
}

fn model_with_rime_words(words: &RimeWords) -> (LanguageModel<'static>, usize) {
    let mut model = DEFAULT_MODEL.clone();
    let mut added = 0;
    for word in &words.words {
        // Keep every existing preset score, rather than flattening 20k English words to cost 6.
        if model.word_cost(word).is_none() {
            model.set_word_cost(word.clone(), DEFAULT_USER_COST);
            added += 1;
        }
    }
    (model, added)
}

fn model_table(lua: &Lua, model: LanguageModel<'static>) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;
    let model = Arc::new(model);
    let splitter = Arc::clone(&model);
    table.set(
        "split",
        lua.create_function(move |_, input: String| Ok(splitter.split(&input).join(" ")))?,
    )?;
    let lookup = Arc::clone(&model);
    table.set(
        "has_word",
        lua.create_function(move |_, word: String| {
            Ok(lookup.word_cost(&word.to_ascii_lowercase()).is_some())
        })?,
    )?;
    table.set(
        "word_cost",
        lua.create_function(
            move |_, word: String| Ok(model.word_cost(&word.to_ascii_lowercase())),
        )?,
    )?;
    Ok(table)
}

fn valid_cost(cost: f64) -> bool {
    cost.is_finite() && cost > 0.0 && cost <= 100.0
}

#[derive(Debug)]
struct UserWords {
    words: HashMap<String, f64>,
    skipped: Vec<usize>,
}

fn parse_words(text: &str, default_cost: f64) -> UserWords {
    let mut result = UserWords {
        words: HashMap::new(),
        skipped: Vec::new(),
    };
    for (index, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let word = fields[0].to_ascii_lowercase();
        let cost = if fields.len() == 1 {
            Some(default_cost)
        } else if fields.len() == 2 {
            fields[1].parse::<f64>().ok()
        } else {
            None
        };
        match cost.filter(|&value| valid_cost(value)) {
            Some(value) if valid_word(&word) => {
                result.words.insert(word, value);
            }
            _ => result.skipped.push(index + 1),
        }
    }
    result
}

fn model_with_words(words: &UserWords) -> LanguageModel<'static> {
    let mut model = DEFAULT_MODEL.clone();
    for (word, cost) in &words.words {
        model.set_word_cost(word.clone(), *cost);
    }
    model
}

#[mlua::lua_module]
fn wordninja(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set(
        "word_cost",
        lua.create_function(|_, word: String| {
            Ok(DEFAULT_MODEL.word_cost(&word.to_ascii_lowercase()))
        })?,
    )?;
    // Dictionary loaders clone the preset model; the module-level split never changes.
    exports.set(
        "load_rime_dir",
        lua.create_function(|lua, path: String| {
            let words = load_rime_directory(Path::new(&path)).map_err(LuaError::external)?;
            let (model, added) = model_with_rime_words(&words);
            let table = model_table(lua, model)?;
            table.set("count", words.words.len())?;
            table.set("added", added)?;
            table.set("preset_duplicates", words.words.len() - added)?;
            table.set("duplicates", words.duplicates)?;
            table.set("files", words.files)?;
            table.set("entries", words.entries)?;
            table.set("skipped", words.skipped)?;
            table.set("source", path)?;
            Ok(table)
        })?,
    )?;
    exports.set(
        "split",
        lua.create_function(|_, input: String| {
            Ok(wordninja::DEFAULT_MODEL.split(&input).join(" "))
        })?,
    )?;
    exports.set(
        "load_words",
        lua.create_function(|lua, (path, default_cost): (String, Option<f64>)| {
            let cost = default_cost.unwrap_or(DEFAULT_USER_COST);
            if !valid_cost(cost) {
                return Err(LuaError::external(
                    "word cost must be finite and in (0, 100]",
                ));
            }
            if fs::metadata(&path).map_err(LuaError::external)?.len() > MAX_FILE_BYTES {
                return Err(LuaError::external("user word file exceeds 1 MiB"));
            }
            let text = fs::read_to_string(&path).map_err(LuaError::external)?;
            let words = parse_words(&text, cost);
            let model = model_with_words(&words);
            let table = lua.create_table()?;
            table.set("count", words.words.len())?;
            table.set("skipped", words.skipped.len())?;
            table.set("skipped_lines", lua.create_sequence_from(words.skipped)?)?;
            table.set("source", path)?;
            table.set("default_cost", cost)?;
            table.set(
                "split",
                lua.create_function(move |_, input: String| Ok(model.split(&input).join(" ")))?,
            )?;
            Ok(table)
        })?,
    )?;
    Ok(exports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rime_columns_and_case_are_deduplicated_without_importing_metadata() {
        let text = "\u{feff}# comment\n---\nname: neverheaderword\ncolumns: [weight, code, text]\n...\n999999\tserde\tSerde\n0\tserde\tSERDE\n0\tstringfrom\tString::from\n";
        let mut words = RimeWords::default();
        parse_rime_dictionary(text, &mut words).unwrap();
        assert_eq!(words.files, 1);
        assert_eq!(words.entries, 3);
        assert_eq!(words.words.len(), 4);
        for word in ["serde", "string", "from", "stringfrom"] {
            assert!(words.words.contains(word));
        }
        assert!(!words.words.contains("999999"));
        assert!(!words.words.contains("neverheaderword"));
        assert!(words.duplicates > 0);
    }

    #[test]
    fn merged_model_preserves_preset_scores() {
        let mut words = RimeWords::default();
        for word in ["the", "Vec", "serde", "VecDeque"] {
            words.insert(word);
        }
        let (model, added) = model_with_rime_words(&words);
        assert_eq!(added, 2);
        assert_eq!(model.word_cost("the"), DEFAULT_MODEL.word_cost("the"));
        assert_eq!(model.word_cost("vec"), DEFAULT_MODEL.word_cost("vec"));
        assert_eq!(model.word_cost("serde"), Some(6.0));
        assert_eq!(model.split("usevecdeque").join(" "), "use vecdeque");
        assert_eq!(model.split("useserde").join(" "), "use serde");
    }

    #[test]
    fn rime_header_errors_are_not_treated_as_vocabulary() {
        for text in [
            "serde\tserde\n",
            "---\nname: a\n",
            "---\ncolumns: [code, weight]\n...\n",
            "---\ncolumns: invalid\n...\n",
        ] {
            assert!(parse_rime_dictionary(text, &mut RimeWords::default()).is_err());
        }
        let mut words = RimeWords::default();
        parse_rime_dictionary(
            "---\ncolumns: [text, weight]\n...\nSerde\t999999\n",
            &mut words,
        )
        .unwrap();
        assert_eq!(words.words.len(), 1);
        assert!(words.words.contains("serde"));
    }

    #[test]
    fn parser_handles_bom_comments_case_duplicates_and_scores() {
        let words = parse_words(
            "\u{feff}# comment\r\nSerde\nserde\t5\nVecDeque 4 # note\n",
            6.0,
        );
        assert_eq!(words.words.len(), 2);
        assert_eq!(words.words["serde"], 5.0);
        assert_eq!(words.words["vecdeque"], 4.0);
        assert!(words.skipped.is_empty());
    }

    #[test]
    fn invalid_words_and_costs_are_reported_by_line() {
        let text =
            "bad-word\nword NaN\nword -1\nword inf\nword 0\nword 101\nword 2 extra\n'\n中文\n";
        let words = parse_words(text, 6.0);
        assert!(words.words.is_empty());
        assert_eq!(words.skipped, (1..=9).collect::<Vec<_>>());
        assert_eq!(parse_words(&"x".repeat(129), 6.0).skipped, vec![1]);
    }

    #[test]
    fn custom_model_changes_only_its_own_scores() {
        let words = parse_words("serde\nvecdeque\nhashmap\nasync\nawait\n", 6.0);
        let model = model_with_words(&words);
        for (input, expected) in [
            ("useserde", "use serde"),
            ("usevecdeque", "use vecdeque"),
            ("usehashmap", "use hashmap"),
            ("asyncawait", "async await"),
        ] {
            assert_eq!(model.split(input).join(" "), expected);
        }
        assert_eq!(DEFAULT_MODEL.split("serde").join(" "), "s erde");
        for input in ["public", "service", "source", "thisisatest", "iloverust"] {
            assert_eq!(model.split(input), DEFAULT_MODEL.split(input));
        }
    }
}
