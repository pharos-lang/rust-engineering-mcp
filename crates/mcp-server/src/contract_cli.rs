//! `rust-engineering-mcp contract [--json | --human]` argument parsing.
use std::ffi::OsString;
use std::process::ExitCode;

pub(crate) enum Invocation {
    Json,
    Human,
}

pub fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let invocation = match args.next() {
        None => Invocation::Json,
        Some(flag) if flag == "--json" => Invocation::Json,
        Some(flag) if flag == "--human" => Invocation::Human,
        Some(_) => return None,
    };
    if args.next().is_some() {
        return None;
    }
    Some(invocation)
}

pub(crate) fn run(invocation: Invocation) -> ExitCode {
    crate::stdio::contract(matches!(invocation, Invocation::Json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_one_optional_format_flag() {
        let parse_values = |values: Vec<&str>| parse(values.into_iter().map(OsString::from));
        assert!(matches!(parse_values(vec![]), Some(Invocation::Json)));
        assert!(matches!(
            parse_values(vec!["--json"]),
            Some(Invocation::Json)
        ));
        assert!(matches!(
            parse_values(vec!["--human"]),
            Some(Invocation::Human)
        ));
        assert!(parse_values(vec!["--unknown"]).is_none());
        assert!(parse_values(vec!["--json", "extra"]).is_none());
        assert!(parse_values(vec!["--json", "--human"]).is_none());
        assert!(parse_values(vec!["--human", "--json"]).is_none());
    }
}
