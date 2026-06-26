use comfy_table::Table;
use serde_json;

use crate::metadata::ProjectMetadata;

pub fn print_table(projects: &[ProjectMetadata]) {
    let mut table = Table::new();
    table.set_header(vec!["PATH", "LANG", "LOC", "COMMITS", "UNPUSHED", "STATUS"]);
    for p in projects {
        let total_loc: u64 = p.languages.iter().map(|l| l.code).sum();
        table.add_row(vec![
            p.path.clone(),
            p.primary_language.clone().unwrap_or_default(),
            total_loc.to_string(),
            p.total_commits.to_string(),
            p.unpushed_count.to_string(),
            p.repostatus_state.clone(),
        ]);
    }
    println!("{table}");
}

pub fn print_json(projects: &[ProjectMetadata]) {
    match serde_json::to_string_pretty(projects) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("JSON serialization error: {e}"),
    }
}

pub fn print_csv(projects: &[ProjectMetadata]) {
    println!("repository,oldest,newest,count");
    for p in projects {
        let fmt = |iso: &Option<String>| {
            iso.as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| {
                    let local: chrono::DateTime<chrono::Local> = dt.into();
                    local.format("%y-%m-%d").to_string()
                })
                .unwrap_or_default()
        };
        println!(
            "{},{},{},{}",
            p.path,
            fmt(&p.oldest_unpushed),
            fmt(&p.newest_unpushed),
            p.unpushed_count,
        );
    }
}

pub fn print_schema() {
    let schema = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ProjectMetadata",
        "type": "object",
        "properties": {
            "path":                    { "type": "string" },
            "name":                    { "type": "string" },
            "is_git":                  { "type": "boolean" },
            "is_worktree":             { "type": "boolean" },
            "has_remote":              { "type": "boolean" },
            "origin_url":              { "type": ["string", "null"] },
            "is_on_github":            { "type": "boolean" },
            "unpushed_count":          { "type": "integer" },
            "oldest_unpushed":         { "type": ["string", "null"] },
            "newest_unpushed":         { "type": ["string", "null"] },
            "branches_with_unpushed":  { "type": "array", "items": { "type": "string" } },
            "total_commits":           { "type": "integer" },
            "primary_language":        { "type": ["string", "null"] },
            "languages": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name":     { "type": "string" },
                        "code":     { "type": "integer" },
                        "comments": { "type": "integer" },
                        "blanks":   { "type": "integer" }
                    }
                }
            },
            "has_readme":              { "type": "boolean" },
            "has_tests":               { "type": "boolean" },
            "has_ci":                  { "type": "boolean" },
            "has_license":             { "type": "boolean" },
            "last_modified":           { "type": ["string", "null"] },
            "repostatus_state":        { "type": "string" },
            "repostatus_age_days":     { "type": ["integer", "null"] }
        }
    });
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
