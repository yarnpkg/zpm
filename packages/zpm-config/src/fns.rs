use zpm_utils::Path;

use crate::ConfigurationContext;

pub fn check_tsconfig(context: &ConfigurationContext) -> bool {
    if let Some(project_cwd) = &context.project_cwd {
        let root_has_tsconfig = project_cwd
            .with_join_str("tsconfig.json")
            .fs_exists_blocking();

        if root_has_tsconfig {
            return true;
        }
    }

    if let Some(package_cwd) = &context.package_cwd {
        let package_has_tsconfig = package_cwd
            .with_join_str("tsconfig.json")
            .fs_exists_blocking();

        if package_has_tsconfig {
            return true;
        }
    }

    false
}

pub fn default_deferred_version_folder(context: &ConfigurationContext) -> Path {
    context
        .project_cwd
        .as_ref()
        .or(context.package_cwd.as_ref())
        .expect("A project or package directory should be set when resolving the default deferred version folder")
        .with_join_str(".yarn/versions")
}
