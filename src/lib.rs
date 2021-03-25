use mlua::prelude::*;

use wordninja::{DEFAULT_MODEL};

fn split(_: &Lua, input: String) -> LuaResult<String> {
    let words: Vec<_> = DEFAULT_MODEL.split(&input);
    let result = words.join(" ");
    Ok(result)
}

#[mlua::lua_module]
fn wordninja(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("split", lua.create_function(split)?)?;
    Ok(exports)
}
