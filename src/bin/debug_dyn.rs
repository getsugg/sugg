fn main() {
    let source = std::fs::read_to_string("tests/fixtures/completions/bun/index.ts").unwrap();
    let (_, pure, _) = sugg::ast::extract_dynamics(&source, "bun/index.ts");
    println!("{}", pure);
}
