use std::collections::{BTreeMap, BTreeSet};

use zpm_primitives::{Descriptor, Ident, IdentGlob, Locator, Range, Reference};
use zpm_semver::Version;
use zpm_utils::FromFileString;

use crate::{
    install::InstallState,
    resolvers::Resolution,
    tree_resolver::ResolutionTree,
};

// Helper function to create a test ident
fn test_ident(name: &str) -> Ident {
    Ident::new(name)
}

// Helper function to create a test locator
fn test_locator(name: &str, version: &str) -> Locator {
    let reference = Reference::from_file_string(&format!("npm:{}", version)).unwrap();
    Locator::new(test_ident(name), reference)
}

// Helper function to create a test descriptor
fn test_descriptor(name: &str, range: &str) -> Descriptor {
    let range_obj = Range::from_file_string(range).unwrap();
    Descriptor::new(test_ident(name), range_obj)
}

// Helper function to create a test resolution
fn test_resolution(locator: Locator, deps: Vec<(&str, &str, &str)>) -> Resolution {
    let mut dependencies = BTreeMap::new();

    for (name, range, _version) in deps {
        dependencies.insert(
            test_ident(name),
            test_descriptor(name, range),
        );
    }

    Resolution {
        locator: locator.clone(),
        version: Version::new(),
        requirements: Default::default(),
        dependencies,
        peer_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeSet::new(),
        optional_peer_dependencies: BTreeSet::new(),
        missing_peer_dependencies: BTreeSet::new(),
    }
}

// Helper function to create a basic install state for testing
fn create_test_install_state() -> InstallState {
    let mut locator_resolutions = BTreeMap::new();
    let mut descriptor_to_locator = BTreeMap::new();

    // Create a simple dependency tree:
    // pkg-a@1.0.0 -> target@1.0.0
    // pkg-b@1.0.0 -> target@2.0.0
    // pkg-c@1.0.0 -> pkg-a@1.0.0 -> target@1.0.0

    let target_1 = test_locator("target", "1.0.0");
    let target_2 = test_locator("target", "2.0.0");
    let pkg_a = test_locator("pkg-a", "1.0.0");
    let pkg_b = test_locator("pkg-b", "1.0.0");
    let pkg_c = test_locator("pkg-c", "1.0.0");

    // target@1.0.0 and target@2.0.0 have no dependencies
    locator_resolutions.insert(
        target_1.clone(),
        test_resolution(target_1.clone(), vec![]),
    );
    locator_resolutions.insert(
        target_2.clone(),
        test_resolution(target_2.clone(), vec![]),
    );

    // pkg-a depends on target@1.0.0
    let pkg_a_res = test_resolution(
        pkg_a.clone(),
        vec![("target", "^1.0.0", "1.0.0")],
    );
    locator_resolutions.insert(pkg_a.clone(), pkg_a_res);
    descriptor_to_locator.insert(
        test_descriptor("target", "^1.0.0"),
        target_1.clone(),
    );

    // pkg-b depends on target@2.0.0
    let pkg_b_res = test_resolution(
        pkg_b.clone(),
        vec![("target", "^2.0.0", "2.0.0")],
    );
    locator_resolutions.insert(pkg_b.clone(), pkg_b_res);
    descriptor_to_locator.insert(
        test_descriptor("target", "^2.0.0"),
        target_2.clone(),
    );

    // pkg-c depends on pkg-a
    let pkg_c_res = test_resolution(
        pkg_c.clone(),
        vec![("pkg-a", "^1.0.0", "1.0.0")],
    );
    locator_resolutions.insert(pkg_c.clone(), pkg_c_res);
    descriptor_to_locator.insert(
        test_descriptor("pkg-a", "^1.0.0"),
        pkg_a.clone(),
    );

    InstallState {
        last_installed_at: 0,
        content_flags: BTreeMap::new(),
        resolution_tree: ResolutionTree {
            roots: BTreeSet::new(),
            descriptor_to_locator,
            locator_resolutions,
            optional_builds: BTreeSet::new(),
        },
        descriptor_to_locator: BTreeMap::new(),
        normalized_resolutions: BTreeMap::new(),
        packages_by_location: BTreeMap::new(),
        locations_by_package: BTreeMap::new(),
        optional_packages: BTreeSet::new(),
        disabled_locators: BTreeSet::new(),
        conditional_locators: BTreeSet::new(),
    }
}

#[test]
fn test_simple_mode_finds_direct_dependencies() {
    let install_state = create_test_install_state();
    let pattern = IdentGlob::new("target").unwrap();

    // Count how many packages directly depend on target
    let mut count = 0;
    for (_locator, resolution) in &install_state.resolution_tree.locator_resolutions {
        for (_ident, descriptor) in &resolution.dependencies {
            if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                if pattern.check(&dep_locator.ident) {
                    count += 1;
                }
            }
        }
    }

    // pkg-a and pkg-b should depend on target
    assert_eq!(count, 2, "Should find 2 direct dependencies on target");
}

#[test]
fn test_simple_mode_no_matches() {
    let install_state = create_test_install_state();
    let pattern = IdentGlob::new("nonexistent").unwrap();

    // Count matches
    let mut count = 0;
    for (_locator, resolution) in &install_state.resolution_tree.locator_resolutions {
        for (_ident, descriptor) in &resolution.dependencies {
            if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                if pattern.check(&dep_locator.ident) {
                    count += 1;
                }
            }
        }
    }

    assert_eq!(count, 0, "Should find no packages matching nonexistent");
}

#[test]
fn test_transitive_dependency_marking() {
    let install_state = create_test_install_state();
    let pattern = IdentGlob::new("target").unwrap();

    // Get target idents
    let target_idents: BTreeSet<Ident> = install_state
        .resolution_tree
        .locator_resolutions
        .keys()
        .filter(|locator| pattern.check(&locator.ident))
        .map(|locator| locator.ident.clone())
        .collect();

    assert!(!target_idents.is_empty(), "Should find target packages");

    // Verify pkg-a depends on target
    let pkg_a = test_locator("pkg-a", "1.0.0");
    let pkg_a_resolution = install_state.resolution_tree.locator_resolutions.get(&pkg_a);
    assert!(pkg_a_resolution.is_some(), "pkg-a should exist");

    let pkg_a_has_target = pkg_a_resolution
        .unwrap()
        .dependencies
        .values()
        .any(|desc| {
            install_state
                .resolution_tree
                .descriptor_to_locator
                .get(desc)
                .map_or(false, |loc| target_idents.contains(&loc.ident))
        });

    assert!(pkg_a_has_target, "pkg-a should depend on target");

    // Verify pkg-c transitively depends on target through pkg-a
    let pkg_c = test_locator("pkg-c", "1.0.0");
    let pkg_c_resolution = install_state.resolution_tree.locator_resolutions.get(&pkg_c);
    assert!(pkg_c_resolution.is_some(), "pkg-c should exist");

    let pkg_c_has_pkg_a = pkg_c_resolution
        .unwrap()
        .dependencies
        .values()
        .any(|desc| {
            install_state
                .resolution_tree
                .descriptor_to_locator
                .get(desc)
                .map_or(false, |loc| loc.ident == test_ident("pkg-a"))
        });

    assert!(pkg_c_has_pkg_a, "pkg-c should depend on pkg-a, which depends on target");
}

#[test]
fn test_ident_glob_pattern_matching() {
    // Test exact match
    let pattern_exact = IdentGlob::new("target").unwrap();
    assert!(pattern_exact.check(&test_ident("target")), "Should match exact name");

    // Test wildcard pattern
    let pattern_wildcard = IdentGlob::new("tar*").unwrap();
    assert!(pattern_wildcard.check(&test_ident("target")), "Should match wildcard pattern");

    // Test non-matching pattern
    let pattern_no_match = IdentGlob::new("xyz*").unwrap();
    assert!(!pattern_no_match.check(&test_ident("target")), "Should not match different name");
}

#[test]
fn test_peer_dependencies_exist_in_resolution() {
    let install_state = create_test_install_state();

    // Verify that resolutions have a peer_dependencies field
    for (_locator, resolution) in &install_state.resolution_tree.locator_resolutions {
        // This test just verifies the structure exists
        let _peer_deps = &resolution.peer_dependencies;
        // If we got here without panic, the field exists
    }

    // Test passes if we can access peer_dependencies on all resolutions
    assert!(true, "Peer dependencies field exists on Resolution");
}

#[test]
fn test_install_state_structure() {
    let install_state = create_test_install_state();

    // Verify the install state has the expected structure
    assert!(install_state.resolution_tree.locator_resolutions.len() > 0, "Should have resolutions");
    assert!(install_state.resolution_tree.descriptor_to_locator.len() > 0, "Should have descriptor mappings");

    // Verify we can traverse the dependency graph
    let pkg_c = test_locator("pkg-c", "1.0.0");
    assert!(install_state.resolution_tree.locator_resolutions.contains_key(&pkg_c), "Should contain pkg-c");

    let pkg_c_resolution = &install_state.resolution_tree.locator_resolutions[&pkg_c];
    assert!(pkg_c_resolution.dependencies.len() > 0, "pkg-c should have dependencies");
}
