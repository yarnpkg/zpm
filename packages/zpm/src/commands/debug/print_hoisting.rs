use clipanion::cli;

use crate::{error::Error, linker::nm::{hoist::{self, Hoister, InputTree, WorkTree}}, project};

#[cli::command]
#[cli::path("debug", "print-hoisting")]
pub struct PrintHoisting {
    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,
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
        hoister.hoist();

        let rendering
            = hoist::TreeRenderer::new(&work_tree).render();

        print!("{}", rendering);

        Ok(())
    }
}
