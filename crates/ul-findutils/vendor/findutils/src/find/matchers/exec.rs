// Copyright 2017 Google Inc.
//
// Use of this source code is governed by a MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

use std::cell::RefCell;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use sysio::io::{self, stderr, Write};

use super::{Matcher, MatcherIO, WalkEntry};

/// Porte pseudo-linus: `std::process::Command` e `argmax::Command` (fork/exec no host) viram este
/// tipo, que guarda argv e cwd e executa pela tabela de programas do pseudo-kernel. `try_arg` imita
/// o limite do argmax: ARG_MAX do Linux (2 MiB) menos a folga de 2048 bytes do POSIX.
struct Command {
    argv: Vec<OsString>,
    cwd: Option<PathBuf>,
    size: usize,
}

struct ExitStatus(i32);

impl ExitStatus {
    fn success(&self) -> bool {
        self.0 == 0
    }
}

const ARG_MAX: usize = 2 * 1024 * 1024 - 2048;

impl Command {
    fn new(program: impl AsRef<OsStr>) -> Self {
        let program = program.as_ref().to_os_string();
        let size = program.len() + 1;
        Self { argv: vec![program], cwd: None, size }
    }

    fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.size += arg.as_ref().len() + 1;
        self.argv.push(arg.as_ref().to_os_string());
        self
    }

    fn try_arg(&mut self, arg: impl AsRef<OsStr>) -> io::Result<&mut Self> {
        if self.size + arg.as_ref().len() + 1 > ARG_MAX {
            return Err(io::Error::from_raw_os_error(7)); // E2BIG
        }
        Ok(self.arg(arg))
    }

    fn try_args<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(&mut self, args: I) -> io::Result<&mut Self> {
        for arg in args {
            self.try_arg(arg)?;
        }
        Ok(self)
    }

    fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    fn status(&mut self) -> io::Result<ExitStatus> {
        let mut cmd = sysio::process::Command::new(&self.argv[0]);
        cmd.args(&self.argv[1..]);
        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }
        // Como o GNU: o filho que não consegue executar diz o motivo e sai com 1; morte por sinal
        // também conta como falha.
        match cmd.status() {
            Ok(st) => Ok(ExitStatus(st.code().unwrap_or(1))),
            Err(e) => {
                let msg = e.raw_os_error().map_or_else(|| e.to_string(), |n| sysio::sysabi::Errno(n).message().clone());
                let _ = writeln!(&mut stderr(), "find: ‘{}’: {msg}", self.argv[0].to_string_lossy());
                Ok(ExitStatus(1))
            }
        }
    }
}

enum Arg {
    FileArg(Vec<OsString>),
    LiteralArg(OsString),
}

fn parse_arg(s: &str) -> Arg {
    let parts = s.split("{}").collect::<Vec<_>>();
    if parts.len() == 1 {
        Arg::LiteralArg(OsString::from(s))
    } else {
        Arg::FileArg(parts.iter().map(OsString::from).collect())
    }
}

pub struct SingleExecMatcher {
    executable: Arg,
    args: Vec<Arg>,
    exec_in_parent_dir: bool,
    interactive: bool,
}

impl SingleExecMatcher {
    pub fn new(
        executable: &str,
        args: &[&str],
        exec_in_parent_dir: bool,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(Self::new_impl(executable, args, exec_in_parent_dir, false))
    }

    pub fn new_interactive(
        executable: &str,
        args: &[&str],
        exec_in_parent_dir: bool,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(Self::new_impl(executable, args, exec_in_parent_dir, true))
    }

    fn new_impl(
        executable: &str,
        args: &[&str],
        exec_in_parent_dir: bool,
        interactive: bool,
    ) -> Self {
        let transformed_args = args.iter().map(|&a| parse_arg(a)).collect();

        Self {
            executable: parse_arg(executable),
            args: transformed_args,
            exec_in_parent_dir,
            interactive,
        }
    }
}

impl Matcher for SingleExecMatcher {
    fn matches(&self, file_info: &WalkEntry, matcher_io: &mut MatcherIO) -> bool {
        let path_to_file = if self.exec_in_parent_dir {
            if let Some(f) = file_info.path().file_name() {
                Path::new(".").join(f)
            } else {
                Path::new(".").join(file_info.path())
            }
        } else {
            file_info.path().to_path_buf()
        };

        let resolved_executable = match self.executable {
            Arg::LiteralArg(ref a) => a.clone(),
            Arg::FileArg(ref parts) => parts.join(path_to_file.as_os_str()),
        };

        if self.interactive {
            // GNU find prints a fixed, abbreviated prompt of the form
            // "< executable ... pathname > ? ".  It does not render the
            // substituted argument list, and always shows the full path of
            // the entry being processed (even for -okdir, whose command runs
            // with the "./basename" form).
            let prompt = format!(
                "< {} ... {} > ? ",
                resolved_executable.to_string_lossy(),
                file_info.path().to_string_lossy()
            );

            if !matcher_io.confirm(&prompt) {
                return false;
            }
        }

        let mut command = Command::new(&resolved_executable);

        for arg in &self.args {
            match *arg {
                Arg::LiteralArg(ref a) => command.arg(a.as_os_str()),
                Arg::FileArg(ref parts) => command.arg(parts.join(path_to_file.as_os_str())),
            };
        }
        if self.exec_in_parent_dir {
            match file_info.path().parent() {
                None => {
                    // Root paths like "/" have no parent.  Run them from the root to match GNU find.
                    command.current_dir(file_info.path());
                }
                Some(parent) if parent == Path::new("") => {
                    // Paths like "foo" have a parent of "".  Avoid chdir("").
                }
                Some(parent) => {
                    command.current_dir(parent);
                }
            }
        }
        match command.status() {
            Ok(status) => status.success(),
            Err(e) => {
                writeln!(
                    &mut stderr(),
                    "Failed to run {}: {}",
                    resolved_executable.to_string_lossy(),
                    e
                )
                .unwrap();
                false
            }
        }
    }

    fn has_side_effects(&self) -> bool {
        true
    }
}

pub struct MultiExecMatcher {
    executable: String,
    args: Vec<OsString>,
    exec_in_parent_dir: bool,
    /// Command to build while matching.
    command: RefCell<Option<Command>>,
}

impl MultiExecMatcher {
    pub fn new(
        executable: &str,
        args: &[&str],
        exec_in_parent_dir: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let transformed_args = args.iter().map(OsString::from).collect();

        Ok(Self {
            executable: executable.to_string(),
            args: transformed_args,
            exec_in_parent_dir,
            command: RefCell::new(None),
        })
    }

    fn new_command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.try_args(&self.args).unwrap();
        command
    }

    fn run_command(&self, command: &mut Command, matcher_io: &mut MatcherIO) {
        match command.status() {
            Ok(status) => {
                if !status.success() {
                    matcher_io.set_exit_code(1);
                }
            }
            Err(e) => {
                writeln!(&mut stderr(), "Failed to run {}: {}", self.executable, e).unwrap();
                matcher_io.set_exit_code(1);
            }
        }
    }
}

impl Matcher for MultiExecMatcher {
    fn matches(&self, file_info: &WalkEntry, matcher_io: &mut MatcherIO) -> bool {
        let path_to_file = if self.exec_in_parent_dir {
            if let Some(f) = file_info.path().file_name() {
                Path::new(".").join(f)
            } else {
                Path::new(".").join(file_info.path())
            }
        } else {
            file_info.path().to_path_buf()
        };
        let mut command = self.command.borrow_mut();
        let command = command.get_or_insert_with(|| self.new_command());

        // Build command, or dispatch it before when it is long enough.
        if command.try_arg(&path_to_file).is_err() {
            if self.exec_in_parent_dir {
                match file_info.path().parent() {
                    None => {
                        // Root paths like "/" have no parent.  Run them from the root to match GNU find.
                        command.current_dir(file_info.path());
                    }
                    Some(parent) if parent == Path::new("") => {
                        // Paths like "foo" have a parent of "".  Avoid chdir("").
                    }
                    Some(parent) => {
                        command.current_dir(parent);
                    }
                }
            }
            self.run_command(command, matcher_io);

            // Reset command status.
            *command = self.new_command();
            if let Err(e) = command.try_arg(&path_to_file) {
                writeln!(
                    &mut stderr(),
                    "Cannot fit a single argument {}: {}",
                    path_to_file.to_string_lossy(),
                    e
                )
                .unwrap();
                matcher_io.set_exit_code(1);
            }
        }
        true
    }

    fn finished_dir(&self, dir: &Path, matcher_io: &mut MatcherIO) {
        // Dispatch command for -execdir.
        if self.exec_in_parent_dir {
            let mut command = self.command.borrow_mut();
            if let Some(mut command) = command.take() {
                command.current_dir(Path::new(".").join(dir));
                self.run_command(&mut command, matcher_io);
            }
        }
    }

    fn finished(&self, matcher_io: &mut MatcherIO) {
        // Dispatch command for -exec.
        if !self.exec_in_parent_dir {
            let mut command = self.command.borrow_mut();
            if let Some(mut command) = command.take() {
                self.run_command(&mut command, matcher_io);
            }
        }
    }

    fn has_side_effects(&self) -> bool {
        true
    }
}

#[cfg(test)]
/// No tests here, because we need to call out to an external executable. See
/// `tests/exec_unit_tests.rs` instead.
mod tests {}
