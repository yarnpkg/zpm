extern crate zpm_macros;
use zpm_macros::Parsed;
use rstest::rstest; // Adjust the path according to your crate's structure.

// This is a mock-up of the enum you provided in your example.
#[derive(Debug, Parsed, PartialEq, Eq)]
enum Range {
    #[pattern = r"^workspace:(.*)"]
    Workspace(String),

    #[pattern = r"(.*\.git$)"]
    Git(String),
}

#[rstest]
#[case("workspace:my_workspace", Range::Workspace("my_workspace".to_string()))]
#[case("git@github.com/my_repo.git", Range::Git("git@github.com/my_repo.git".to_string()))]
fn test_workspace_pattern(#[case] input: &str, #[case] expected: Range) {
    assert_eq!(Range::from_str(input).unwrap(), expected);
}
