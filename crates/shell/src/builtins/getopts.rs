//! `getopts optstring nome [args]`.

use sysabi::Fd;

use crate::shell::{Exec, Shell, write_fd};

impl Shell {
    fn optind(&mut self) -> usize {
        self.get_scalar("OPTIND")
            .and_then(|v| std::str::from_utf8(&v).ok().and_then(|s| s.trim().parse::<i64>().ok()))
            .filter(|n| *n >= 1)
            .unwrap_or(1) as usize
    }
}

pub fn getopts(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if argv.len() < 3 {
        sh.builtin_error("getopts", "usage: getopts optstring name [arg ...]");
        let _ = write_fd(Fd::STDERR, b"getopts: usage: getopts optstring name [arg ...]\n");
        return Ok(2);
    }
    let optstring = argv[1].clone();
    let name = String::from_utf8_lossy(&argv[2]).into_owned();
    if !crate::word::is_name(name.as_bytes()) {
        sh.builtin_error("getopts", format!("`{name}': not a valid identifier"));
        return Ok(1);
    }
    let args: Vec<Vec<u8>> = if argv.len() > 3 { argv[3..].to_vec() } else { sh.params.clone() };
    let silent = optstring.first() == Some(&b':');
    let spec = if silent { &optstring[1..] } else { &optstring[..] };
    let opterr = sh.get_scalar("OPTERR").is_none_or(|v| v != b"0");

    let mut optind = sh.optind();
    // A posição dentro de um grupo (`-abc`) só vale se o OPTIND não mudou por fora.
    if sh.getopts_state.0 != optind {
        sh.getopts_state = (optind, 1);
    }
    let mut charpos = sh.getopts_state.1;

    let finish = |sh: &mut Shell, optind: usize| -> Exec {
        sh.assign_scalar("OPTIND", optind.to_string().into_bytes(), false)?;
        sh.assign_scalar(&name, b"?".to_vec(), false)?;
        sh.getopts_state = (optind, 1);
        Ok(1)
    };
    let Some(arg) = args.get(optind - 1).cloned() else {
        return finish(sh, optind);
    };
    if charpos == 1 {
        if arg.len() < 2 || arg[0] != b'-' {
            return finish(sh, optind);
        }
        if arg == b"--" {
            return finish(sh, optind + 1);
        }
    }
    let c = arg[charpos];
    charpos += 1;
    let at_end = charpos >= arg.len();
    let pos = spec.iter().position(|x| *x == c && c != b':');
    let prog = String::from_utf8_lossy(&sh.arg0).into_owned();
    let advance = |optind: &mut usize, charpos: &mut usize, at_end: bool| {
        if at_end {
            *optind += 1;
            *charpos = 1;
        }
    };
    match pos {
        None => {
            if silent {
                sh.assign_scalar("OPTARG", vec![c], false)?;
            } else {
                if opterr {
                    let _ = write_fd(Fd::STDERR, format!("{prog}: illegal option -- {}\n", c as char).as_bytes());
                }
                sh.unset_var("OPTARG");
            }
            advance(&mut optind, &mut charpos, at_end);
            sh.assign_scalar(&name, b"?".to_vec(), false)?;
        }
        Some(p) => {
            let takes_arg = spec.get(p + 1) == Some(&b':');
            if takes_arg {
                let value = if !at_end {
                    let v = arg[charpos..].to_vec();
                    optind += 1;
                    charpos = 1;
                    Some(v)
                } else {
                    optind += 1;
                    charpos = 1;
                    match args.get(optind - 1) {
                        Some(v) => {
                            optind += 1;
                            Some(v.clone())
                        }
                        None => None,
                    }
                };
                match value {
                    Some(v) => {
                        sh.assign_scalar("OPTARG", v, false)?;
                        sh.assign_scalar(&name, vec![c], false)?;
                    }
                    None => {
                        if silent {
                            sh.assign_scalar("OPTARG", vec![c], false)?;
                            sh.assign_scalar(&name, b":".to_vec(), false)?;
                        } else {
                            if opterr {
                                let _ = write_fd(Fd::STDERR, format!("{prog}: option requires an argument -- {}\n", c as char).as_bytes());
                            }
                            sh.unset_var("OPTARG");
                            sh.assign_scalar(&name, b"?".to_vec(), false)?;
                        }
                    }
                }
            } else {
                sh.unset_var("OPTARG");
                advance(&mut optind, &mut charpos, at_end);
                sh.assign_scalar(&name, vec![c], false)?;
            }
        }
    }
    sh.assign_scalar("OPTIND", optind.to_string().into_bytes(), false)?;
    sh.getopts_state = (optind, charpos);
    Ok(0)
}
