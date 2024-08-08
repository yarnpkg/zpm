mod exec;
mod install;
mod node;
mod default;
mod run;

clipanion::program!(YarnCli, [
    exec::Exec,
    install::Install,
    default::Default,
    node::Node,
    run::Run,
]);
