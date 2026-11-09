//! Catalog identity rules shared by the publisher CLI and the master service.
//!
//! Two manifest-supplied names cross the wire and end up as directory names: `mod_id` is the
//! master service's object-store key (`<mod_id>/<mod_id>.pbo.zst`) and `folderName` is every
//! client's install directory (`<modsRoot>/<folderName>`). The engine applies
//! `Foundation::IsSinglePathComponent`
//! (`engine/Poseidon/Foundation/Common/Filenames.hpp`) before either is joined onto a
//! directory, so publishing, serving and installing all validate with the rule below. Keeping
//! it here — the one crate both ends already depend on — means the publisher cannot accept a
//! name the service refuses, or the other way round.

/// True when `name` is exactly one relative path component: a name that means the same folder
/// on every supported filesystem. Non-empty, not `.` or `..`, no `/` or `\` (so no separator,
/// no absolute path, no UNC root), no drive/stream `:`, no control characters, no
/// Windows-reserved `*` `?` `"` `<` `>` `|`, and no trailing `.` or ` ` (Windows trims those,
/// so `".. "` would collapse back to `..`).
///
/// This is a port of `Foundation::IsSinglePathComponent`, deliberately down to the character
/// ranges: C1 (`U+0080..=U+009F`) is allowed because the engine allows it, which is why this
/// does not use `char::is_control`. A name the engine would refuse to install must not be
/// publishable, and a name it installs must not be refused.
pub fn is_single_path_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    name.chars().all(|ch| {
        // C0 and DEL, the engine's exact range.
        let code = u32::from(ch);
        !(code < 0x20
            || code == 0x7f
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    })
}

/// Normalize a manifest's `folderName` the way both ends of the workshop do: trim it, treat an
/// absent or whitespace-only value as "no folder name" (`None`, so installs fall back to
/// `@<modId>`), and otherwise require one path component. The value is not a store key but the
/// directory every client installs into (`<modsRoot>/<folderName>`), and the install
/// transaction renames and then removes that directory.
pub fn normalize_folder_name(raw: Option<&str>) -> Result<Option<String>, InvalidFolderName> {
    let Some(name) = raw.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    if is_single_path_component(name) {
        Ok(Some(name.to_string()))
    } else {
        Err(InvalidFolderName(name.to_string()))
    }
}

/// A `folderName` that is not one path component, carrying the trimmed value. `Display` is the
/// message the master service and the publisher both report, so the two cannot word it
/// differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidFolderName(pub String);

impl std::fmt::Display for InvalidFolderName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid folder name '{}'", self.0)
    }
}

impl std::error::Error for InvalidFolderName {}

#[cfg(test)]
mod tests {
    use super::{is_single_path_component, normalize_folder_name};

    // The catalog-name boundary is one fixture for both languages: this test, the C++ case
    // `ModInstallDir refuses catalog names that are not one path component`
    // (tests/unit/engine/Poseidon/Core/test_mod_install.cpp) and
    // `publish_holds_both_manifest_names_to_the_engine_component_set`
    // (mserver/MasterService/src/mods.rs) all read that file, so a reserved character added
    // there fails every suite at once instead of being re-typed per language.
    fn catalog_name_cases(key: &str) -> Vec<String> {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/mod_names/catalog_name_cases.json"
        ));
        let json: serde_json::Value =
            serde_json::from_str(fixture).expect("the catalog name cases fixture is valid JSON");
        json[key]
            .as_array()
            .unwrap_or_else(|| panic!("'{key}' is an array in the fixture"))
            .iter()
            .map(|case| {
                case.as_str()
                    .unwrap_or_else(|| panic!("'{key}' holds strings"))
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn the_component_rule_matches_the_engine() {
        let accepted = catalog_name_cases("accepted");
        let refused = catalog_name_cases("refused");
        // A list that parsed to nothing would pass vacuously, so both sides carry a floor.
        assert!(accepted.len() >= 8, "accepted cases: {accepted:?}");
        assert!(refused.len() >= 20, "refused cases: {refused:?}");

        for name in &accepted {
            assert!(is_single_path_component(name), "refused {name:?}");
        }
        for name in &refused {
            assert!(!is_single_path_component(name), "accepted {name:?}");
        }

        // Not a fixture case: blank gets no single verdict. An empty `folderName` means "no
        // folder name" (installs fall back to `@<modId>`) while an empty `modId` is refused.
        assert!(!is_single_path_component(""));

        // C1 stays allowed: `char::is_control()` would reject it and drift from the engine.
        assert!(is_single_path_component("a\u{85}b"));
    }

    // The normalization rule that used to live only in the master service: absent or blank
    // means "no folder name" (clients fall back to `@<modId>`), otherwise the trimmed value
    // must be one component. The error carries the trimmed value and words itself, so the CLI
    // and the service cannot report different messages.
    #[test]
    fn folder_names_normalize_or_carry_one_message() {
        for absent in [None, Some(""), Some("   "), Some("\t\n")] {
            assert_eq!(normalize_folder_name(absent), Ok(None), "{absent:?}");
        }

        for (raw, expected) in [
            ("  CSLA  ", "CSLA"),
            ("@CSLA", "@CSLA"),
            ("Sth..else", "Sth..else"),
            ("ver2.0", "ver2.0"),
        ] {
            assert_eq!(
                normalize_folder_name(Some(raw)),
                Ok(Some(expected.to_string())),
                "{raw:?}"
            );
        }

        for raw in [
            "..",
            ".. ",
            ".",
            "nested/path",
            "C:\\mods",
            "a*b",
            "trailing.",
        ] {
            let refused = normalize_folder_name(Some(raw)).unwrap_err();
            let trimmed = raw.trim();
            assert_eq!(refused.0, trimmed, "{raw:?}");
            assert_eq!(
                refused.to_string(),
                format!("invalid folder name '{trimmed}'"),
                "{raw:?}"
            );
            let _: &dyn std::error::Error = &refused;
        }
    }
}
