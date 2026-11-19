//! Checks the hand-maintained contract field names against the frozen UDL.
//!
//! `contract_field_names.rs` names the lower-camel JavaScript fields the
//! hand-written canvas and vector layer read. The constants are
//! hand-maintained, so an unused one fails the wasm32 build while a *stale*
//! one would not: the canvas would simply read a property the generated
//! records no longer carry. This test parses the frozen contract and fails
//! when a constant names a record or field the UDL does not declare, or when
//! its value is not the field's lower-camel spelling.

use std::collections::{BTreeMap, BTreeSet};

/// The module that holds the constants, read as text so its doc comments can
/// be matched to the declarations.
const CONSTANTS_SOURCE: &str = include_str!("../src/contract_field_names.rs");

/// The frozen contract the generated browser API is built from.
const FROZEN_CONTRACT: &str = include_str!("../../rustic-maps-native-core/src/rustic_maps.udl");

/// One constant with the contract field its documentation names.
struct DeclaredField<'a> {
    rust_name: &'a str,
    record_name: &'a str,
    field_name: &'a str,
    javascript_name: &'a str,
}

/// Extracts the record and field from a constant's documentation line.
fn documented_field(line: &str) -> Option<(&str, &str)> {
    let marker = "JavaScript field name of the contract's `";
    let start = line.find(marker)? + marker.len();
    let rest = line.get(start..)?;
    let end = rest.find('`')?;
    let qualified_name = &rest[..end];
    qualified_name.split_once('.')
}

/// Reads every constant together with the record field it documents.
fn declared_fields(source: &str) -> Vec<DeclaredField<'_>> {
    let mut declared = Vec::new();
    let mut pending_documentation: Option<(&str, &str)> = None;
    for line in source.lines() {
        if let Some(field) = documented_field(line) {
            pending_documentation = Some(field);
            continue;
        }
        if !line.trim_start().starts_with("pub(crate) const") {
            continue;
        }
        let documented = pending_documentation.take().unwrap_or_else(|| {
            panic!("constant without a documented contract field: {line}");
        });
        let rust_name = line
            .split_whitespace()
            .nth(2)
            .and_then(|name| name.strip_suffix(':'))
            .unwrap_or_else(|| panic!("unreadable constant declaration: {line}"));
        let javascript_name = line
            .split('"')
            .nth(1)
            .unwrap_or_else(|| panic!("constant without a quoted value: {line}"));
        declared.push(DeclaredField {
            rust_name,
            record_name: documented.0,
            field_name: documented.1,
            javascript_name,
        });
    }
    assert!(
        pending_documentation.is_none(),
        "a documented field has no constant beneath it",
    );
    declared
}

/// Reads the field names of every dictionary the contract declares.
fn contract_dictionary_fields(contract: &str) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut dictionaries = BTreeMap::new();
    let mut current_dictionary: Option<&str> = None;
    for line in contract.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("dictionary ") {
            let name = rest
                .split_whitespace()
                .next()
                .unwrap_or_else(|| panic!("unreadable dictionary declaration: {line}"));
            dictionaries.insert(name, BTreeSet::new());
            current_dictionary = Some(name);
            continue;
        }
        if trimmed == "};" {
            current_dictionary = None;
            continue;
        }
        let dictionary_name = match current_dictionary {
            Some(name) => name,
            None => continue,
        };
        if trimmed.is_empty() || trimmed.starts_with("///") || trimmed.starts_with('}') {
            continue;
        }
        let Some(field_declaration) = trimmed.strip_suffix(';') else {
            continue;
        };
        // A default follows the name (`double? tokens = null;`); drop it
        // before reading the field's own identifier.
        let without_default = field_declaration
            .split('=')
            .next()
            .expect("splitting on '=' always yields a first part");
        let field_name = without_default
            .split_whitespace()
            .last()
            .unwrap_or_else(|| panic!("unreadable dictionary field: {line}"));
        dictionaries
            .get_mut(dictionary_name)
            .expect("the dictionary is registered before its fields")
            .insert(field_name);
    }
    dictionaries
}

/// Spells a snake_case contract field the way the generated records do.
fn lower_camel(snake_case_name: &str) -> String {
    let mut spelled = String::with_capacity(snake_case_name.len());
    let mut uppercase_next_character = false;
    for character in snake_case_name.chars() {
        if character == '_' {
            uppercase_next_character = true;
            continue;
        }
        if uppercase_next_character {
            spelled.extend(character.to_uppercase());
            uppercase_next_character = false;
        } else {
            spelled.push(character);
        }
    }
    spelled
}

#[test]
fn every_constant_names_a_contract_field_and_its_lower_camel_spelling() {
    let dictionaries = contract_dictionary_fields(FROZEN_CONTRACT);
    let declared = declared_fields(CONSTANTS_SOURCE);
    assert!(
        !declared.is_empty(),
        "no constants were found in contract_field_names.rs",
    );

    let mut differences = Vec::new();
    for constant in &declared {
        let Some(fields) = dictionaries.get(constant.record_name) else {
            differences.push(format!(
                "{} names the record `{}`, which the contract does not declare",
                constant.rust_name, constant.record_name,
            ));
            continue;
        };
        if !fields.contains(constant.field_name) {
            differences.push(format!(
                "{} names `{}.{}`, which the contract does not declare",
                constant.rust_name, constant.record_name, constant.field_name,
            ));
            continue;
        }
        let expected_javascript_name = lower_camel(constant.field_name);
        if constant.javascript_name != expected_javascript_name {
            differences.push(format!(
                "{} spells `{}.{}` as `{}` instead of `{}`",
                constant.rust_name,
                constant.record_name,
                constant.field_name,
                constant.javascript_name,
                expected_javascript_name,
            ));
        }
    }

    assert!(
        differences.is_empty(),
        "the contract field names drifted from the frozen UDL:\n{}",
        differences.join("\n"),
    );
}
