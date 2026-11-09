//! `ahp`: solves classic AHP and AHPSort II projects stored in `.ahp` files.

mod questionnaire;
mod report;
mod table;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ahp::{Evaluations, Method, Project, quizzes, solve_classic, solve_short};
use clap::{ArgAction, Args, Parser, Subcommand};

use questionnaire::Prompter;

const HELP_TEMPLATE: &str = "{about-with-newline}\nUso: {usage}\n\n{all-args}{after-help}";

#[derive(Parser, Debug)]
#[command(
    name = "ahp",
    version,
    about = "Resuelve problemas de decisión AHP clásico y AHPSort II a partir de ficheros .ahp.",
    help_template = HELP_TEMPLATE,
    subcommand_help_heading = "Comandos",
    subcommand_value_name = "COMANDO",
    next_help_heading = "Opciones",
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Muestra esta ayuda
    #[arg(short = 'h', long = "help", action = ArgAction::Help, global = true)]
    help: Option<bool>,

    /// Muestra la versión
    #[arg(short = 'V', long = "version", action = ArgAction::Version)]
    version: Option<bool>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Resuelve el proyecto con AHP clásico y muestra el ranking de alternativas
    #[command(
        help_template = HELP_TEMPLATE,
        next_help_heading = "Opciones",
        override_usage = "ahp classic [OPCIONES] <FICHERO>"
    )]
    Classic(RunArgs),
    /// Resuelve el proyecto con AHPSort II y muestra la clasificación de alternativas
    #[command(
        name = "shortII",
        alias = "short-ii",
        help_template = HELP_TEMPLATE,
        next_help_heading = "Opciones",
        override_usage = "ahp shortII [OPCIONES] <FICHERO>"
    )]
    ShortII(RunArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Fichero de proyecto .ahp
    #[arg(value_name = "FICHERO", help_heading = "Argumentos")]
    file: PathBuf,

    /// Muestra solo el resultado final (ranking o clasificación)
    #[arg(long = "no-verbose")]
    no_verbose: bool,

    /// Guarda el proyecto con las evaluaciones completas en este fichero
    #[arg(long, value_name = "SALIDA")]
    save: Option<PathBuf>,
}

fn criterion_header(method: Method, name: &str, unit: &str) -> String {
    let what = match method {
        Method::Classic => "comparación de alternativas",
        Method::ShortII => "comparación de puntos de referencia",
    };
    if unit.trim().is_empty() {
        format!("Criterio «{name}»: {what}")
    } else {
        format!("Criterio «{name}» ({unit}): {what}")
    }
}

/// Asks, through stdin/stderr, only the evaluations that `from_project` reported missing.
fn complete_evaluations(project: &Project, method: Method) -> Result<Evaluations, String> {
    let (mut ev, missing) =
        Evaluations::from_project(project, method).map_err(|e| e.to_string())?;
    if missing.is_complete() {
        return Ok(ev);
    }
    let (criteria_quiz, per_criterion) = quizzes(project, method).map_err(|e| e.to_string())?;
    let mut prompter = Prompter::new(io::stdin().lock(), io::stderr().lock());
    if missing.criteria {
        ev.criteria = prompter
            .ask("Comparación de criterios", &criteria_quiz)
            .map_err(|e| e.to_string())?;
    }
    for (i, (name, quiz)) in per_criterion.iter().enumerate() {
        if !missing.per_criterion.get(i).copied().unwrap_or(false) {
            continue;
        }
        let unit = project
            .criteria
            .get(i)
            .map(|c| c.unit.as_str())
            .unwrap_or("");
        let header = criterion_header(method, name, unit);
        ev.per_criterion[i] = prompter.ask(&header, quiz).map_err(|e| e.to_string())?;
    }
    Ok(ev)
}

fn save(project: &mut Project, ev: &Evaluations, method: Method, out: &Path) -> Result<(), String> {
    ev.store_into(project, method);
    if project.method.is_none() {
        project.method = Some(method);
    }
    project
        .save(out)
        .map_err(|e| format!("No se pudo guardar «{}»: {e}", out.display()))?;
    eprintln!("Proyecto guardado en «{}».", out.display());
    Ok(())
}

fn run(method: Method, args: RunArgs) -> Result<String, String> {
    let mut project = Project::load(&args.file)
        .map_err(|e| format!("No se pudo cargar «{}»: {e}", args.file.display()))?;
    let ev = complete_evaluations(&project, method)?;
    let verbose = !args.no_verbose;
    let text = match method {
        Method::Classic => {
            let outcome = solve_classic(&project, &ev).map_err(|e| e.to_string())?;
            report::classic(&project, &outcome, verbose)
        }
        Method::ShortII => {
            let outcome = solve_short(&project, &ev).map_err(|e| e.to_string())?;
            report::short(&project, &outcome, verbose)
        }
    };
    // Save only after the solver accepted the project, so a rejected state is never written.
    if let Some(out) = &args.save {
        save(&mut project, &ev, method, out)?;
    }
    Ok(text)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (method, args) = match cli.command {
        Command::Classic(args) => (Method::Classic, args),
        Command::ShortII(args) => (Method::ShortII, args),
    };
    match run(method, args) {
        Ok(text) => {
            let mut stdout = io::stdout().lock();
            match stdout
                .write_all(text.as_bytes())
                .and_then(|()| stdout.flush())
            {
                Ok(()) => ExitCode::SUCCESS,
                // A closed pipe (e.g. `| head`) is not an error for the user.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("Error: no se pudo escribir el resultado: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(msg) => {
            eprintln!("Error: {msg}");
            ExitCode::FAILURE
        }
    }
}
