use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn output_ui_path(project_root: &Path, input_rel: &Path) -> PathBuf {
    let file_name = input_rel
        .file_name()
        .unwrap_or_else(|| panic!("Invalid blueprint path: {}", input_rel.display()));

    let mut out = project_root.join("resources/ui").join(file_name);
    out.set_extension("ui");
    out
}

fn compile_blp(project_root: &Path, input_rel: &Path) {
    let input_path = project_root.join(input_rel);
    let output_path = output_ui_path(project_root, input_rel);

    println!("cargo:rerun-if-changed={}", input_path.display());

    if !input_path.exists() {
        panic!("Blueprint input file not found: {}", input_path.display());
    }

    if input_path.extension().and_then(|s| s.to_str()) != Some("blp") {
        panic!(
            "Only .blp files are allowed in blp-resources, got: {}",
            input_path.display()
        );
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("Failed to create {}: {e}", parent.display()));
    }

    let status = Command::new("blueprint-compiler")
        .arg("compile")
        .arg("--output")
        .arg(&output_path)
        .arg(&input_path)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "Failed to execute blueprint-compiler for {}: {e}\n\
                 Make sure `blueprint-compiler` is installed and available in PATH.",
                input_path.display()
            )
        });

    if !status.success() {
        panic!(
            "blueprint-compiler failed for {}\noutput file: {}",
            input_path.display(),
            output_path.display()
        );
    }
}

fn read_blp_resources(project_root: &Path) -> Vec<PathBuf> {
    let manifest = project_root.join("blp-resources.in");

    println!("cargo:rerun-if-changed={}", manifest.display());

    let content = fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", manifest.display()));

    content
        .lines()
        .enumerate()
        .filter_map(|(idx, raw)| {
            let line = raw.trim();

            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let path = PathBuf::from(line);

            if path.is_absolute() {
                panic!(
                    "Absolute paths are not allowed in blp-resources (line {}): {}",
                    idx + 1,
                    line
                );
            }

            Some(path)
        })
        .collect()
}

fn check_duplicate_outputs(project_root: &Path, inputs: &[PathBuf]) {
    let mut seen: HashMap<PathBuf, PathBuf> = HashMap::new();

    for input in inputs {
        let output = output_ui_path(project_root, input);

        if let Some(previous) = seen.insert(output.clone(), input.clone()) {
            panic!(
                "Duplicate UI output path detected:\n  {}\nfrom:\n  {}\n  {}",
                output.display(),
                previous.display(),
                input.display()
            );
        }
    }
}

fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"),
    );

    let inputs = read_blp_resources(&manifest_dir);

    if inputs.is_empty() {
        println!("cargo:warning=blp-resources is empty");
    }

    check_duplicate_outputs(&manifest_dir, &inputs);

    for input in inputs {
        compile_blp(&manifest_dir, &input);
    }
}
