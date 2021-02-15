/**
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.*
 */
use std::collections::{HashMap, HashSet};
use std::fs::{self};
use std::fs::{read_to_string, write};
use std::path::Path;
use std::process::Command;

use os_pipe::{dup_stderr, dup_stdout};
use serde_json::{from_str, json, to_string_pretty, Value};

fn main() {
    clone_schema_repo();
    merge_schema();
}

fn clone_schema_repo() {
    Command::new("rm")
        .arg("-rf")
        .arg("gateway-addon-ipc-schema")
        .stdout(dup_stdout().expect("Could not redirect stdout"))
        .stderr(dup_stderr().expect("Could not redirect stderr"))
        .output()
        .expect("Could not delete old schema repo");

    Command::new("git")
        .arg("clone")
        .arg("https://github.com/WebThingsIO/gateway-addon-ipc-schema.git")
        .stdout(dup_stdout().expect("Could not redirect stdout"))
        .stderr(dup_stderr().expect("Could not redirect stderr"))
        .output()
        .expect("Could not clone schema repo");

    Command::new("git")
        .arg("-C")
        .arg("gateway-addon-ipc-schema")
        .arg("checkout")
        .arg("v1.0.0")
        .stdout(dup_stdout().expect("Could not redirect stdout"))
        .stderr(dup_stderr().expect("Could not redirect stderr"))
        .output()
        .expect("Could not checkout correct schema version");
}

fn merge_schema() {
    let root = "gateway-addon-ipc-schema/messages";

    let mut definitions = HashMap::new();
    let mut visited = HashSet::new();

    for entry in fs::read_dir(root).expect(&format!("Could not read dir {}", root)) {
        let entry = entry.unwrap();
        let path = entry.path();
        let path = path.to_str().unwrap();

        let file_name = Path::new(path).file_stem().unwrap().to_str().unwrap();

        if file_name != "definitions" {
            let data = read_to_string(path).expect(&format!("Could not read {}", path));
            let value: Value = from_str(&data).expect(&format!("Could not parse {}", path));
            let base_path = Path::new(path).parent().unwrap().to_str().unwrap();

            let value = resolve_object(
                base_path,
                base_path,
                None,
                value,
                &mut definitions,
                &mut visited,
            );

            definitions.insert(String::from(file_name), value);

            println!("Processing {}", path);
        }
    }

    let mut schema: HashMap<String, HashMap<String, Value>> = HashMap::new();
    schema.insert(String::from("definitions"), definitions);

    write("schema.json", to_string_pretty(&schema).unwrap()).unwrap();
}

fn resolve_object(
    root_path: &str,
    base_path: &str,
    current_file: Option<&str>,
    value: Value,
    definitions: &mut HashMap<String, Value>,
    visited: &mut HashSet<String>,
) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map: HashMap<String, Value> = HashMap::new();

            for (k, v) in map {
                let new_value = match &*k {
                    "$ref" => {
                        replace_ref(root_path, base_path, current_file, v, definitions, visited)
                    }
                    _ => {
                        resolve_object(root_path, base_path, current_file, v, definitions, visited)
                    }
                };

                new_map.insert(k, new_value);
            }

            match (new_map.get("type"), new_map.get("enum")) {
                (Some(Value::String(data_type)), Some(Value::Array(_))) => match &data_type[..] {
                    "integer" => {
                        new_map.remove("enum");
                    }
                    "number" => {
                        new_map.remove("enum");
                    }
                    _ => {}
                },
                _ => {}
            }

            json!(new_map)
        }
        Value::Array(arr) => {
            let mut new_arr: Vec<Value> = Vec::new();
            for v in arr {
                let new_value =
                    resolve_object(root_path, base_path, current_file, v, definitions, visited);
                new_arr.push(new_value);
            }
            json!(new_arr)
        }
        x => x,
    }
}

fn replace_ref(
    root_path: &str,
    base_path: &str,
    current_file: Option<&str>,
    value: Value,
    definitions: &mut HashMap<String, Value>,
    visited: &mut HashSet<String>,
) -> Value {
    match value {
        Value::String(ref_string) => {
            let mut ref_parts = ref_string.split("#").collect::<Vec<&str>>();
            ref_parts.reverse();
            let file_option = ref_parts.pop().filter(|x| x.chars().count() > 0);
            let path_option = ref_parts.pop();

            match file_option.or(current_file) {
                Some(relative_file_path) => {
                    let absolute_file_path = Path::new(base_path).join(relative_file_path);
                    let content_string = read_to_string(absolute_file_path)
                        .expect(&format!("Could not read {}", relative_file_path));
                    let absolute_file_path = Path::new(base_path).join(relative_file_path);
                    let reference = absolute_file_path.strip_prefix(root_path).unwrap();
                    let reference_parts = reference
                        .iter()
                        .map(|x| x.to_str().unwrap())
                        .collect::<Vec<&str>>();

                    if file_option.is_some() {
                        let absolute_file_path_string = absolute_file_path.to_str().unwrap();

                        if !visited.contains(absolute_file_path_string) {
                            visited.insert(String::from(absolute_file_path_string));

                            let content_value: Value = from_str(&content_string).unwrap();
                            let absolute_file_path = Path::new(base_path).join(relative_file_path);
                            let base_path = absolute_file_path.parent().unwrap().to_str().unwrap();

                            let resolved_value = resolve_object(
                                root_path,
                                base_path,
                                Some(relative_file_path),
                                content_value,
                                definitions,
                                visited,
                            );

                            let mut reversed_parts = reference_parts.clone();
                            reversed_parts.reverse();

                            insert(&mut reversed_parts, resolved_value, definitions);
                        }
                    }

                    let mut ref_string = String::from("#/definitions");

                    if reference_parts.capacity() > 0 {
                        ref_string.push_str("/");
                        ref_string.push_str(&reference_parts.join("/"));
                    }

                    match path_option {
                        Some(p) => {
                            ref_string.push_str(p);
                        }
                        None => {}
                    }

                    json!(ref_string)
                }
                None => {
                    json!(ref_string)
                }
            }
        }
        x => x,
    }
}

fn insert(parts: &mut Vec<&str>, value: Value, map: &mut HashMap<String, Value>) -> Value {
    let key_option = parts.pop();

    match key_option {
        Some(key) => {
            let mut new_map: HashMap<String, Value> = HashMap::new();

            let new_value = match map.remove(key) {
                Some(existing_value) => match existing_value {
                    Value::Object(existing_map) => {
                        let mut replaced_map: HashMap<String, Value> = HashMap::new();

                        for (k, v) in existing_map {
                            replaced_map.insert(k, v);
                        }

                        insert(parts, value, &mut replaced_map);
                        json!(replaced_map)
                    }
                    x => x,
                },
                None => insert(parts, value, &mut new_map),
            };

            map.insert(String::from(key), new_value);

            json!(map)
        }
        None => value,
    }
}
