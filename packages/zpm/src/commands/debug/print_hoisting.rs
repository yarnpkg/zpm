use clipanion::cli;
use zpm_utils::tree;

use crate::{error::Error, linker::nm::hoist::{self, Hoister, InputTree, WorkTree}, project};

#[cli::command]
#[cli::path("debug", "print-hoisting")]
pub struct PrintHoisting {
    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    #[cli::option("-j,--json", default = false)]
    json: bool,
}

impl PrintHoisting {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .import_install_state()?;

        let install_state
            = project.install_state.as_ref().unwrap();

        let input_tree
            = InputTree::from_install_state(&project, install_state);

        let mut work_tree
            = WorkTree::from_input_tree(&input_tree);

        let mut hoister
            = Hoister::new(&mut work_tree);

        hoister.set_print_logs(self.verbose);
        println!("Hoisting...");
        hoister.hoist();
        println!("Hoisted!");

        let root_node
            = hoist::TreeRenderer::new(&work_tree).convert();

        let rendering
            = tree::TreeRenderer::new()
                .render(&root_node, self.json);

        print!("{}", rendering);

        Ok(())
    }
}
