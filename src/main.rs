use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;

use clap::{Parser, Subcommand};
use dashmap::{DashMap, DashSet};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};

// Function to extract "package <some.value>;"
fn extract_package(
    file_path: &Path,
) -> Option<String> {
    let file_content = fs::read_to_string(file_path).ok()?;

    let package_regex = regex::Regex::new(r"package\s+(\S+);").ok()?;

    let captures = package_regex.captures(&file_content)?;
    let package_name = captures.get(1)?;

    Some(package_name.as_str().to_string())
}

// Function to extract all "import <some.value>;"
fn extract_imports(
    file_path: &Path,
) -> Option<Vec<String>> {
    let file_content = fs::read_to_string(file_path).ok()?;

    let import_regex = regex::Regex::new(r"import\s+(\S+);").ok()?;

    return Some(
        import_regex
            .captures_iter(&file_content)
            .map(|captures| captures[1].to_string())
            .collect()
    );
}

// Function to build the dependency tree
fn build_dependency_tree(
    imports_map: &DashMap<String, Vec<String>>,
    root_class_prefix: Option<&str>,
    depth: Option<usize>,
) -> DashMap<String, Vec<String>> {
    let mut tree = DashMap::<String, Vec<String>>::new();
    let mut visited = DashSet::<String>::new();

    let mut stack = Vec::new();

    let depth = depth.unwrap_or(usize::MAX);

    for package_name in imports_map.iter()
        .map(|entry| entry.key().to_string()) {
        if root_class_prefix.is_none() {
            stack.push((package_name.to_string(), 0));
        }

        if let Some(root_class_prefix) = root_class_prefix {
            if package_name.starts_with(root_class_prefix) {
                stack.push((package_name.to_string(), 0));
            }
        }
    }

    while let Some((package_name, current_depth)) = stack.pop() {
        if current_depth > depth {
            continue;
        }

        if !visited.contains(&package_name) {
            visited.insert(package_name.clone());

            tree.entry(package_name.clone()).or_insert(Vec::new());

            if let Some(imports) = imports_map.get(&package_name) {
                for import_value in imports.iter() {
                    tree.get_mut(&package_name)
                        .unwrap()
                        .push(import_value.to_string());

                    stack.push(
                        (
                            import_value.clone(),
                            current_depth + 1,
                        ),
                    );
                }
            }
        }
    }

    tree
}

// Function to generate the dot content
fn generate_dot_content(
    imports_map: &DashMap<String, Vec<String>>,
    root_class_prefix: Option<&str>,
    depth: Option<usize>,
    rank_dir: RankDir,
) -> String {
    let mut dot_content = String::new();
    dot_content += "strict digraph G {\n";

    match rank_dir {
        RankDir::LR => dot_content += "  rankdir=LR;\n",
        RankDir::RL => dot_content += "  rankdir=RL;\n",
        RankDir::TB => dot_content += "  rankdir=TB;\n",
        RankDir::BT => dot_content += "  rankdir=BT;\n",
    }

    dot_content += "  graph [bgcolor=black];\n";
    dot_content += "  graph [label=\"Orthogonal edges\", splines=ortho, nodesep=0.8];\n";
    dot_content += "  edge [color=white];\n";
    dot_content += "  graph[ratio=fill,center=1];\n";
    dot_content += "  node[style=filled, shape=box];\n";

    let dependency_tree =
        build_dependency_tree(
            imports_map,
            root_class_prefix,
            depth,
        );

    for (package_name, imports) in dependency_tree {
        for import_value in imports {
            dot_content += &format!(
                "  \"{}\" -> \"{}\";\n",
                package_name.replace('"', "'").replace('/', "_"),
                import_value.replace('"', "'").replace('/', "_")
            );
        }
    }

    dot_content += "}";

    dot_content
}

fn traverse_folder(
    folder_path: PathBuf,
) -> DashMap<String, Vec<String>> {
    let mut imports_map =
        DashMap::<String, Vec<String>>::new();

    let mut stack =
        vec![folder_path.to_path_buf()];

    while let Some(path) = stack.pop() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let file_path = entry.path();
                    let metadata = fs::metadata(&file_path).unwrap();

                    if metadata.is_dir() {
                        stack.push(file_path);
                    } else if metadata.is_file() {
                        // check if the file is a java file
                        if let Some(extension) = file_path.extension() {
                            if extension != "java" {
                                continue;
                            }
                        } else {
                            continue;
                        }

                        if let Some(package_name) = extract_package(&file_path) {
                            let imports =
                                extract_imports(&file_path)
                                    .unwrap_or(Vec::new());

                            imports_map.insert(package_name, imports);
                        }
                    }
                }
            }
        }
    }

    imports_map
}

fn traverse_folder_par(
    folder_path: PathBuf,
) -> DashMap<String, Vec<String>> {
    let imports_map: DashMap<String, Vec<String>> = DashMap::new();
    let stack: Vec<PathBuf> = vec![folder_path.to_path_buf()];

    stack.par_iter().for_each(|path| {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                let metadata = fs::metadata(&file_path).unwrap();
                if metadata.is_file() {
                    // check if the file is a java file
                    if let Some(extension) = file_path.extension() {
                        if extension == "java" {
                            if let Some(package_name) = extract_package(&file_path) {
                                let imports =
                                    extract_imports(&file_path)
                                        .unwrap_or(Vec::new());

                                imports_map.insert(package_name, imports);
                            }
                        }
                    }
                } else if metadata.is_dir() {
                    for (key, value) in traverse_folder_par(file_path) {
                        imports_map.insert(key, value);
                    }
                }
            }
        }
    });

    imports_map
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Serialize, Deserialize, Debug)]
enum RankDir {
    #[serde(rename = "lr")]
    LR,
    #[serde(rename = "rl")]
    RL,
    #[serde(rename = "tb")]
    TB,
    #[serde(rename = "bt")]
    BT,
}

impl FromStr for RankDir {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "lr" => Ok(RankDir::LR),
            "rl" => Ok(RankDir::RL),
            "tb" => Ok(RankDir::TB),
            "bt" => Ok(RankDir::BT),
            _ => Err(()),
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a graphviz graph from a folder of java files
    Graph {
        /// Path to folder containing java files
        #[arg(short, long, value_name = "PATH")]
        path: String,

        /// Graphviz output file name; defaults to "<prefix>.svg" if not provided
        #[arg(short, long, value_name = "OUTPUT_FILE_NAME")]
        graph_out: Option<String>,

        /// Optional root class prefix to use as starting point
        #[arg(short, long, value_name = "ROOT_CLASS_PREFIX")]
        class_prefix: Option<String>,

        /// Optional depth of classes to traverse if root class prefix is provided
        #[arg(short, long, value_name = "DEPTH")]
        depth: Option<usize>,

        /// Optional rank direction
        #[arg(short, long, value_name = "RANK_DIR")]
        rank_dir: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Graph {
            path,
            graph_out,
            class_prefix,
            depth,
            rank_dir,
        } => {
            let folder_path = Path::new(path.as_str());
            let root_class_prefix = class_prefix;
            let depth = depth;
            let rank_dir: RankDir =
                RankDir::from_str(
                    rank_dir.unwrap_or("lr".to_string()).as_str(),
                ).unwrap();

            let svg_file_path =
                if let Some(ref root_class_prefix) = root_class_prefix {
                    if let Some(graph_out) = graph_out {
                        Path::new(graph_out.as_str()).to_path_buf()
                    } else {
                        Path::new(
                            format!("{}.svg", root_class_prefix).as_str(),
                        )
                            .to_path_buf()
                    }
                } else {
                    if let Some(graph_out) = graph_out {
                        Path::new(graph_out.as_str()).to_path_buf()
                    } else {
                        Path::new("graph.svg").to_path_buf()
                    }
                };

            let mut imports_map: DashMap<String, Vec<String>> =
                traverse_folder_par(folder_path.to_path_buf());

            println!("Found {} packages", imports_map.len());

            if let Some(ref root_class_prefix) = root_class_prefix {
                imports_map.insert(
                    root_class_prefix.to_string(),
                    imports_map
                        .iter()
                        .map(|entry| entry.key().to_string())
                        .filter(|package_name| package_name.starts_with(&*root_class_prefix))
                        .map(|package_name| package_name.to_string())
                        .collect(),
                );
            }

            let dot_content =
                generate_dot_content(
                    &imports_map,
                    root_class_prefix.as_deref(),
                    depth,
                    rank_dir,
                );

            let mut dot_process = Command::new("dot")
                .arg("-Tsvg")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();

            println!("Generating svg file...");

            if let Some(stdin) = dot_process.stdin.as_mut() {
                stdin.write_all(dot_content.as_bytes()).unwrap();
                drop(stdin);
            }

            let mut svg_file = fs::File::create(svg_file_path).unwrap();

            if let Ok(output) = dot_process.wait_with_output() {
                let mut stdout = std::io::BufReader::new(output.stdout.as_slice());
                std::io::copy(&mut stdout, &mut svg_file).unwrap();
            }
        }
    }
}
