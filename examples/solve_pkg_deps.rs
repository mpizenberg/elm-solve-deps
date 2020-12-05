use dirs;
use pubgrub::solver::resolve;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use ureq;

use pubgrub_dependency_provider_elm::dependency_provider::{
    ElmPackageProviderOffline, ElmPackageProviderOnline,
};
use pubgrub_dependency_provider_elm::pkg_version::PkgVersion;

fn main() {
    let arg = std::env::args().skip(1).next().unwrap();
    let pkg_version = PkgVersion::from_str(&arg).unwrap();
    let author = &pkg_version.author_pkg.author;
    let pkg = &pkg_version.author_pkg.pkg;
    let offline_deps_provider = ElmPackageProviderOffline::new(elm_home(), "0.19.1");
    let online_deps_provider = ElmPackageProviderOnline::new(
        elm_home(),
        "0.19.1",
        "https://package.elm-lang.org",
        http_fetch,
    );
    match resolve(
        &offline_deps_provider,
        format!("{}/{}", author, pkg),
        pkg_version.version,
    ) {
        Ok(all_deps) => {
            let mut all_deps_formatted: Vec<_> = all_deps
                .iter()
                .map(|(p, v)| format!("{}@{}", p, v))
                .collect();
            all_deps_formatted.sort();
            eprintln!("{:#?}", all_deps_formatted)
        }
        Err(err) => eprintln!("{:?}", err),
    }
}

fn elm_home() -> PathBuf {
    match std::env::var_os("ELM_HOME") {
        None => default_elm_home(),
        Some(os_string) => os_string.into(),
    }
}

#[cfg(target_family = "unix")]
fn default_elm_home() -> PathBuf {
    dirs::home_dir()
        .expect("Unknown home directory")
        .join(".elm")
}

#[cfg(target_family = "windows")]
fn default_elm_home() -> PathBuf {
    dirs::data_dir()
        .expect("Unknown data directory")
        .join("elm")
}

fn http_fetch(url: &str) -> Result<String, Box<dyn Error>> {
    ureq::get(url)
        .timeout_connect(10_000)
        .call()
        .into_string()
        .map_err(|e| e.into())
}
