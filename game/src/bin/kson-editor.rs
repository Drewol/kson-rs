pub fn main() -> anyhow::Result<()> {
    if let Err(e) = kson_editor::main() {
        Err(anyhow::anyhow!("{}", e.to_string()))
    } else {
        Ok(())
    }
}
