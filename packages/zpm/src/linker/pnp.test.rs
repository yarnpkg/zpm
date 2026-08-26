use std::{collections::{BTreeMap, BTreeSet}, str::FromStr};

use zpm_primitives::{Ident, Reference};

use super::*;

fn sample_state() -> PnpState {
    let mut package_registry_data = BTreeMap::new();
    let mut package_information = BTreeMap::new();
    let root_reference = Reference::from_str("workspace:.").unwrap();

    package_information.insert(Some(PnpReference(Locator::new(
        Ident::new("root"),
        root_reference.clone(),
    ))), PnpPackageInformation {
        package_location: "./".to_string(),
        package_dependencies: BTreeMap::new(),
        package_peers: Vec::new(),
        link_type: PackageLinking::Soft,
        discard_from_lookup: false,
    });

    package_registry_data.insert(Some(Ident::new("root")), package_information);

    let mut fallback_exclusion_list = BTreeMap::new();
    fallback_exclusion_list.insert(
        Ident::new("root"),
        BTreeSet::from([PnpReference(Locator::new(
            Ident::new("root"),
            root_reference.clone(),
        ))]),
    );

    PnpState {
        enable_top_level_fallback: true,
        fallback_pool: Vec::new(),
        fallback_exclusion_list,
        ignore_pattern_data: Some(vec!["foo'\\bar\nbaz".to_string()]),
        package_registry_data,
        dependency_tree_roots: vec![PnpDependencyTreeRoot {
            name: Ident::new("root"),
            reference: root_reference,
        }],
    }
}

fn prev_inline_script(shebang: &str, state: &PnpState) -> Result<Vec<u8>, Error> {
    let script = vec![
        shebang, "\n",
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "const RAW_RUNTIME_STATE =\n",
        &single_quote_stringify(&JsonDocument::to_string_pretty(state)?), ";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  return hydrateRuntimeState(JSON.parse(RAW_RUNTIME_STATE), {basePath: basePath || __dirname});\n",
        "}\n",
        &misc::unpack_brotli_data(PNP_CJS_TEMPLATE)?,
    ].join("");

    Ok(script.into_bytes())
}

fn prev_split_setup_script(shebang: &str) -> Result<Vec<u8>, Error> {
    let script = vec![
        shebang, "\n",
        "/* eslint-disable */\n",
        "// @ts-nocheck\n",
        "\"use strict\";\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  const fs = require('fs');\n",
        "  const path = require('path');\n",
        "  const pnpDataFilepath = path.resolve(__dirname, '.pnp.data.json');\n",
        "  return hydrateRuntimeState(JSON.parse(fs.readFileSync(pnpDataFilepath, 'utf8')), {basePath: basePath || __dirname});\n",
        "}\n",
        &misc::unpack_brotli_data(PNP_CJS_TEMPLATE)?,
    ].join("");

    Ok(script.into_bytes())
}

#[test]
fn inline_builder_matches_legacy_output() {
    let state = sample_state();
    let expected = prev_inline_script("#!/usr/bin/env node", &state).unwrap();
    let actual = build_inline_script_bytes("#!/usr/bin/env node", &state).unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn split_setup_builder_matches_legacy_output() {
    let expected = prev_split_setup_script("#!/usr/bin/env node").unwrap();
    let actual = build_split_setup_script_bytes("#!/usr/bin/env node").unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn split_data_builder_matches_legacy_output() {
    let state = sample_state();
    let expected = JsonDocument::to_string(&state).unwrap().into_bytes();
    let actual = build_split_data_bytes(&state).unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn loader_template_matches_legacy_output() {
    let expected = misc::unpack_brotli_data(PNP_MJS_TEMPLATE).unwrap();
    let actual = pnp_loader_template().unwrap();

    assert_eq!(actual, expected);
}
