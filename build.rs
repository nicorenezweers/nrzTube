// Die Oberfläche (Dioxus, `ui/`) wird mit `scripts/ui.sh` nach `ui/dist/public` gebaut und mit
// rust-embed ins Programm eingebettet. Fehlt der Ordner, legt dieses Skript ihn leer an, damit
// der Server trotzdem übersetzt (er zeigt dann einen Hinweis statt der Oberfläche).
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/dist/public");
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    println!("cargo:rerun-if-changed=ui/dist/public");
}
