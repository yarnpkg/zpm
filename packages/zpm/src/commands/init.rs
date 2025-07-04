use clipanion::cli;
use zpm_utils::{Path, ToFileString};

use crate::{
  error::Error, 
  manifest::Manifest,
  primitives::Ident,
  project,
  script::ScriptEnvironment,
};

#[cli::command]
#[cli::path("init")]
#[derive(Debug)]
pub struct Init {
  #[cli::option("-p,--private", default = false)]
  private: bool,
  
  #[cli::option("-w,--workspace", default = false)]
  workspace: bool,
  
  #[cli::option("-n,--name")]
  name: Option<String>,
  
  // Hidden legacy options
  #[cli::option("-2", default = false)]
  usev2: bool,
  
  #[cli::option("-y,--yes", default = false)]
  yes: bool,
}

impl Init {
  #[tokio::main(flavor = "current_thread")]
  pub async fn execute(&self) -> Result<(), Error> {
    // Get current working directory
    let cwd
      = Path::current_dir()?;
    
    // Create directory if it doesn't exist
    if !cwd.fs_exists() {
      cwd.fs_create_dir_all()?;
    }
    
    // Try to find existing project
    let existing_project
      = project::Project::find_closest_project(cwd.clone()).ok();
    
    // Create or load manifest
    let manifest_path
      = cwd.with_join_str("package.json");
    
    let mut manifest = if manifest_path.fs_exists() {
      // Try to load existing manifest
      let content = manifest_path.fs_read_text()?;
      sonic_rs::from_str::<Manifest>(&content)?
    } else {
      Manifest::default()
    };
    
    // Set package name
    manifest.name = self.name.as_ref()
      .map(|n| Ident::new(n))
      .or_else(|| {
        let basename = cwd.basename()
          .unwrap_or("package");

        Some(Ident::new(basename))
      });
    
    // Set private flag
    if self.private {
      manifest.private = Some(true);
    }
    
    // Set up workspace
    if self.workspace && manifest.workspaces.is_none() {
      let packages_dir = cwd
        .with_join_str("packages");
      
      packages_dir
        .fs_create_dir_all()?;
      
      manifest.workspaces = Some(vec![
        "packages/*".to_string(),
      ]);
    }
      
    // Write manifest
    let manifest_json
      = sonic_rs::to_string_pretty(&manifest)?;
      
    manifest_path
      .fs_write_text(&format!("{}\n", manifest_json))?;
      
    let mut changed_paths = vec![
      manifest_path.clone(),
    ];
      
    // Create README.md
    let readme_path = cwd.with_join_str("README.md");
    if !readme_path.fs_exists() {
      if let Some(name) = manifest.name.as_ref() {
        let readme_content
          = format!("# {}\n", name.as_str());
          
        readme_path
          .fs_write_text(&readme_content)?;
          
        changed_paths.push(readme_path.clone());
      }
    }
      
    // Only create lockfile and other files if we're in the project root
    let is_project_root = existing_project
      .as_ref()
      .map(|(project_cwd, _)| project_cwd == &cwd)
      .unwrap_or(true);
      
    if is_project_root {
      // Create yarn.lock
      let lockfile_path = cwd
        .with_join_str("yarn.lock");
        
      if !lockfile_path.fs_exists() {
        lockfile_path
          .fs_write_text("")?;
          
        changed_paths.push(
          lockfile_path.clone(),
        );
      }
        
      // Create .gitignore
      let gitignore_path = cwd
        .with_join_str(".gitignore");
        
      if !gitignore_path.fs_exists() {
        let gitignore_content = vec![
          ".yarn/ignore/*\n",
          "\n",
          "# Whether you use PnP or not, the node_modules folder is often used to store\n",
          "# build artifacts that should be gitignored\n",
          "node_modules\n",
        ];
          
        gitignore_path
          .fs_write_text(&gitignore_content.join(""))?;
          
        changed_paths.push(
          gitignore_path.clone(),
        );
      }
        
      // Create .gitattributes
      let gitattributes_path = cwd
        .with_join_str(".gitattributes");
        
      if !gitattributes_path.fs_exists() {
        let gitattributes_content = vec![
          "/.yarn/**            linguist-vendored\n",
          "/.yarn/releases/*    binary\n",
          "/.pnp.*              binary linguist-generated\n",
        ];
          
        gitattributes_path
          .fs_write_text(&gitattributes_content.join(""))?;
          
        changed_paths.push(
          gitattributes_path.clone(),
        );
      }
        
      // Create .editorconfig
      let editorconfig_path = cwd
        .with_join_str(".editorconfig");
        
      if !editorconfig_path.fs_exists() {
        let editorconfig_content = vec![
          "root = true\n",
          "\n",
          "[*]\n",
          "charset = utf-8\n",
          "end_of_line = lf\n",
          "indent_size = 2\n",
          "indent_style = space\n",
          "insert_final_newline = true\n",
        ];
          
        editorconfig_path
          .fs_write_text(&editorconfig_content.join(""))?;
          
        changed_paths.push(
          editorconfig_path.clone(),
        );
      }
        
      // Initialize git repository
      let git_path = cwd
        .with_join_str(".git");
        
      if !git_path.fs_exists() {
        // git init
        let init = ScriptEnvironment::new()?
          .run_exec("git", ["init"])
          .await?
          .ok();

        if init.is_ok() {
          // git add
          let mut add_args = vec!["add", "--"];

          let changed_path_strings = changed_paths.iter()
            .map(|path| path.to_file_string())
            .collect::<Vec<_>>();

          add_args.extend(changed_path_strings.iter().map(|s| s.as_str()));

          ScriptEnvironment::new()?
            .run_exec("git", add_args)
            .await?
            .ok()?;
            
          // git commit
          ScriptEnvironment::new()?
            .run_exec("git", ["commit", "--allow-empty", "-m", "First commit"])
            .await?
            .ok()?;
        }
      }
    }
      
    Ok(())
  }
}
