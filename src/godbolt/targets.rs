use super::GodboltMode;

use crate::types::{Context, Data};

use anyhow::{anyhow, Error};
use poise::serenity_prelude as serenity;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GodboltTarget {
    id: String,
    name: String,
    semver: String,
    instruction_set: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GodboltLibraryVersion {
    #[allow(unused)]
    id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(unused)]
struct GodboltLibrary {
    #[allow(unused)]
    id: String,
    #[allow(unused)]
    versions: Vec<GodboltLibraryVersion>,
}

#[derive(Default, Debug)]
pub struct GodboltMetadata {
    targets: Vec<GodboltTarget>,
    #[allow(unused)]
    libraries: Vec<GodboltLibrary>,
    last_update_time: Option<std::time::Instant>,
}

impl GodboltTarget {
    fn clean_request_data(&mut self) {
        // Some semvers get weird characters like `()` in them or spaces, we strip that out here
        self.semver = self
            .semver
            .chars()
            .filter(|char| char.is_alphanumeric() || matches!(char, '.' | '-' | '_'))
            .map(|char| char.to_ascii_lowercase())
            .collect();
    }
}

async fn update_godbolt_metadata(
    data: &crate::types::Data,
    lang: &'static str,
) -> Result<(), Error> {
    let last_update_time = match lang {
        "rust" => data.godbolt_rust_targets.lock().unwrap().last_update_time,
        "c++" => data.godbolt_cpp_targets.lock().unwrap().last_update_time,
        &_ => todo!(),
    };

    let needs_update = if let Some(last_update_time) = last_update_time {
        // Get the time to wait between each update of the godbolt metadata
        let update_period = std::env::var("GODBOLT_UPDATE_DURATION")
			.ok()
			.and_then(|duration| duration.parse::<u64>().ok())
			.map(std::time::Duration::from_secs)
			// Currently set to 12 hours
			.unwrap_or_else(|| std::time::Duration::from_secs(60 * 60 * 12));

        let time_since_update =
            std::time::Instant::now().saturating_duration_since(last_update_time);
        let needs_update = time_since_update >= update_period;
        if needs_update {
            log::info!(
                "godbolt metadata was last updated {:#?} ago, updating it",
                time_since_update,
            );
        }

        needs_update
    } else {
        log::info!("godbolt metadata hasn't yet been updated, fetching it");

        true
    };

    // If we should perform an update then do so
    if needs_update {
        let request = data
            .http
            .get(format!("https://godbolt.org/api/compilers/{}", lang))
            .header(reqwest::header::ACCEPT, "application/json");
        let mut targets: Vec<GodboltTarget> = request.send().await?.json().await?;
        // Clean up the data we've gotten from the request
        if lang == "rust" {
            for target in &mut targets {
                target.clean_request_data();
                if let Some(semver) = target.semver.strip_prefix("rustc ") {
                    target.semver = semver.to_owned();
                }
            }
        } else if lang == "c++" {
            // Clean up the data we've gotten from the request
            for target in &mut targets {
                target.clean_request_data();
            }
        }

        let godbolt_targets = match lang {
            "rust" => &data.godbolt_rust_targets,
            "c++" => &data.godbolt_cpp_targets,
            &_ => todo!(),
        };

        let request = data
            .http
            .get(format!("https://godbolt.org/api/libraries/{}", lang))
            .header(reqwest::header::ACCEPT, "application/json");
        let response = request.send().await?;
        let libraries: Vec<GodboltLibrary> = response.json().await?;

        log::info!(
            "updating {} godbolt metadata: {} targets, {} libraries",
            lang,
            targets.len(),
            libraries.len()
        );
        *godbolt_targets.lock().unwrap() = GodboltMetadata {
            targets,
            libraries,
            last_update_time: Some(std::time::Instant::now()),
        };
    }

    Ok(())
}

async fn fetch_godbolt_rust_metadata(
    data: &Data,
) -> impl std::ops::Deref<Target = GodboltMetadata> + '_ {
    // If we encounter an error while updating the targets list, just log it
    if let Err(error) = update_godbolt_metadata(data, "rust").await {
        log::error!("failed to update godbolt targets list: {:?}", error);
    }

    data.godbolt_rust_targets.lock().unwrap()
}

async fn fetch_godbolt_cpp_metadata(
    data: &Data,
) -> impl std::ops::Deref<Target = GodboltMetadata> + '_ {
    // If we encounter an error while updating the targets list, just log it
    if let Err(error) = update_godbolt_metadata(data, "c++").await {
        log::error!("failed to update godbolt targets list: {:?}", error);
    }

    data.godbolt_cpp_targets.lock().unwrap()
}

// Generates godbolt-compatible compiler identifier and flags from command input
pub(super) async fn compiler_id_and_flags(
    data: &Data,
    params: &poise::KeyValueArgs,
    language: &str,
    mode: GodboltMode,
) -> Result<(String, String), Error> {
    match language {
        "rust" => rustc_id_and_flags(data, params, mode).await,
        "c++" => cpp_id_and_flags(data, params, mode).await,
        &_ => Err(anyhow::anyhow!("Unsupported language. Valid: 'rust', 'c++'.")),
    }
}

// Generates godbolt-compatible rustc identifier and flags from command input
//
// Transforms human readable rustc version (e.g. "1.34.1") into compiler id on godbolt (e.g.
// "r1341") Full list of version<->id can be obtained at https://godbolt.org/api/compilers/rust
async fn rustc_id_and_flags(
    data: &Data,
    params: &poise::KeyValueArgs,
    mode: GodboltMode,
) -> Result<(String, String), Error> {
    let rustc = params.get("rustc").unwrap_or("nightly");
    let targets = fetch_godbolt_rust_metadata(data).await.targets.clone();
    let target =
        targets.into_iter().find(|target| target.semver == rustc.trim()).ok_or(anyhow::anyhow!(
            "the `rustc` argument should be a version specifier like `nightly` `beta` or \
             `1.45.2`. Run ?targets_rust for a full list",
        ))?;

    let mut flags = params.get("flags").unwrap_or("-Copt-level=3 --edition=2021").to_owned();
    if mode == GodboltMode::LlvmIr {
        flags += " --emit=llvm-ir -Cdebuginfo=0";
    }

    Ok((target.id, flags))
}

// Generates godbolt-compatible rustc identifier and flags from command input
//
// Transforms human readable rustc version (e.g. "1.34.1") into compiler id on godbolt (e.g.
// "r1341") Full list of version<->id can be obtained at https://godbolt.org/api/compilers/c++
async fn cpp_id_and_flags(
    data: &Data,
    params: &poise::KeyValueArgs,
    _mode: GodboltMode,
) -> Result<(String, String), Error> {
    let cppcompiler = params.get("compiler").unwrap_or("clang_trunk");
    let targets = fetch_godbolt_cpp_metadata(data).await.targets.clone();
    let target = targets.into_iter().find(|target| target.id == cppcompiler.trim()).ok_or(
        anyhow::anyhow!(
            "the `compiler` argument should be a id specifier like `clang_trunk` `gsnapshot` or \
             `g122`. Run ?targets_cpp  for a full list",
        ),
    )?;

    let flags = params.get("flags").unwrap_or("-std=c++20 -O3").to_owned();

    Ok((target.id, flags))
}

/// Used to rank godbolt compiler versions for listing them out
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SemverRanking<'a> {
    Beta,
    Nightly,
    Compiler(&'a str),
    Semver(std::cmp::Reverse<(u16, u16, u16)>),
}

impl<'a> From<&'a str> for SemverRanking<'a> {
    fn from(semver: &'a str) -> Self {
        match semver {
            "beta" => Self::Beta,
            "nightly" => Self::Nightly,

            semver => {
                // Rustc versions are received in a `X.X.X` form, so we parse out
                // the major/minor/patch versions and then order them in *reverse*
                // order based on their version triple, this means that the most
                // recent (read: higher) versions will be at the top of the list
                let mut version_triple = semver.splitn(3, '.');
                let version_triple = version_triple
                    .next()
                    .zip(version_triple.next())
                    .zip(version_triple.next())
                    .and_then(|((major, minor), patch)| {
                        Some((major.parse().ok()?, minor.parse().ok()?, patch.parse().ok()?))
                    });

                // If we successfully parsed out a semver tuple, return it
                if let Some((major, minor, patch)) = version_triple {
                    Self::Semver(std::cmp::Reverse((major, minor, patch)))

                // Anything that doesn't fit the `X.X.X` format we treat as an alternative
                // compiler, we list these after beta & nightly but before the many canonical
                // rustc versions
                } else {
                    Self::Compiler(semver)
                }
            },
        }
    }
}

/// Lists all available godbolt rustc targets
#[poise::command(prefix_command, slash_command, broadcast_typing, category = "Godbolt")]
pub async fn targets_rust(ctx: Context<'_>) -> Result<(), Error> {
    let mut targets = fetch_godbolt_rust_metadata(ctx.data()).await.targets.clone();

    // Can't use sort_by_key because https://github.com/rust-lang/rust/issues/34162
    targets.sort_unstable_by(|lhs, rhs| {
        SemverRanking::from(&*lhs.semver).cmp(&SemverRanking::from(&*rhs.semver))
    });

    ctx.send(poise::CreateReply::default().embed(
        serenity::CreateEmbed::default().title("Godbolt Rust Targets").fields(
            targets.into_iter().map(|target| {
                (
                    target.semver,
                    format!("{} (runs on {})", target.name, target.instruction_set),
                    true,
                )
            }),
        ),
    ))
    .await?;

    Ok(())
}

/// Lists all available godbolt cpp targets
#[poise::command(prefix_command, category = "Godbolt", broadcast_typing, track_edits)]
pub async fn targets_cpp(ctx: Context<'_>) -> Result<(), Error> {
    let mut targets = fetch_godbolt_cpp_metadata(ctx.data()).await.targets.clone();

    // Can't use sort_by_key because https://github.com/rust-lang/rust/issues/34162
    targets.sort_unstable_by(|lhs, rhs| {
        SemverRanking::from(&*lhs.semver).cmp(&SemverRanking::from(&*rhs.semver))
    });

    ctx.send(poise::CreateReply::default().embed(
        serenity::CreateEmbed::default().title("Godbolt C++ Targets").fields(
            targets.into_iter().map(|target| {
                (
                    target.semver,
                    format!("{} (runs on {})", target.name, target.instruction_set),
                    true,
                )
            }),
        ),
    ))
    .await?;

    Ok(())
}
