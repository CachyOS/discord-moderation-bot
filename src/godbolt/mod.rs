mod targets;
use targets::compiler_id_and_flags;
pub use targets::{targets_cpp, targets_rust, GodboltMetadata};

use crate::{Context, Error};

const LLVM_MCA_TOOL_ID: &str = "llvm-mcatrunk";

enum Compilation {
    Success { asm: String, stdout: String, stderr: String, llvm_mca: Option<String> },
    Error { stderr: String },
}

#[derive(Debug, serde::Deserialize)]
struct GodboltOutputSegment {
    text: String,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltOutput(Vec<GodboltOutputSegment>);

impl GodboltOutput {
    pub fn concatenate(&self) -> String {
        let mut complete_text = String::new();
        for segment in self.0.iter() {
            complete_text.push_str(&segment.text);
            complete_text.push('\n');
        }
        complete_text
    }
}

#[derive(Debug, serde::Deserialize)]
struct GodboltResponse {
    code: i8,
    // stdout: GodboltOutput,
    stderr: GodboltOutput,
    asm: GodboltOutput,
    tools: Vec<GodboltTool>,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltRunResponse {
    code: i8,
    stdout: GodboltOutput,
    stderr: GodboltOutput,
    #[serde(rename = "buildResult")]
    build_result: GodboltBuildResult,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltBuildResult {
    stderr: GodboltOutput,
}

#[derive(Debug, serde::Deserialize)]
struct GodboltTool {
    id: String,
    // code: u8,
    stdout: GodboltOutput,
    // stderr: GodboltOutput,
}

/// Execute a given source code file on Godbolt
/// Returns a multiline string
async fn run_cpp_source(
    http: &reqwest::Client,
    source_code: &str,
    compiler: &str,
    flags: &str,
) -> Result<Compilation, Error> {
    let request = http
        .post(&format!(
            "https://godbolt.org/api/compiler/{}/compile",
            compiler
        ))
        .header(reqwest::header::ACCEPT, "application/json") // to make godbolt respond in JSON
        .json(&serde_json::json! { {
            "source": source_code,
            "compiler": compiler,
            "lang": "c++",
            "allowStoreCodeDebug": true,
            "options": {
                "userArguments": flags,
                "compilerOptions": { "executorRequest": true, },
                "filters": { "execute": true, },
                "tools": [],
                "libraries": [
                    {"id": "curl", "version": "7831"},
                    {"id": "range-v3", "version": "trunk"},
                    {"id": "fmt", "version": "trunk"}
                ],
            },
        } })
        .build()?;

    let response: GodboltRunResponse = http.execute(request).await?.json().await?;

    // TODO: use the extract_relevant_lines utility to strip stderr nicely
    Ok(if response.code == 0 {
        Compilation::Success {
            stdout: response.stdout.concatenate(),
            stderr: response.stderr.concatenate(),
            asm: String::new(),
            llvm_mca: None,
        }
    } else {
        Compilation::Error { stderr: response.build_result.stderr.concatenate() }
    })
}

/// Compile a given source code file on Godbolt using the latest nightly compiler with
/// full optimizations (-O3)
/// Returns a multiline string with the pretty printed assembly
async fn compile_source(
    http: &reqwest::Client,
    source_code: &str,
    compiler: &str,
    flags: &str,
    language: &str,
    run_llvm_mca: bool,
) -> Result<Compilation, Error> {
    let tools = if run_llvm_mca {
        serde_json::json! {
            [{"id": LLVM_MCA_TOOL_ID}]
        }
    } else {
        serde_json::json! {
            []
        }
    };

    let libraries = if language == "c++" {
        serde_json::json! {[
            {"id": "curl", "version": "7831"},
            {"id": "range-v3", "version": "trunk"},
            {"id": "fmt", "version": "trunk"}
        ]}
    } else {
        serde_json::json! {
            []
        }
    };

    let request = http
        .post(&format!(
            "https://godbolt.org/api/compiler/{}/compile",
            compiler
        ))
        .header(reqwest::header::ACCEPT, "application/json") // to make godbolt respond in JSON
        .json(&serde_json::json! { {
            "source": source_code,
            "options": {
                "userArguments": flags,
                "tools": tools,
                "libraries": libraries,
            },
        } })
        .build()?;

    let response: GodboltResponse = http.execute(request).await?.json().await?;

    // TODO: use the extract_relevant_lines utility to strip stderr nicely
    Ok(if response.code == 0 {
        Compilation::Success {
            asm: response.asm.concatenate(),
            stderr: response.stderr.concatenate(),
            stdout: String::new(),
            llvm_mca: match response.tools.iter().find(|tool| tool.id == LLVM_MCA_TOOL_ID) {
                Some(llvm_mca) => Some(llvm_mca.stdout.concatenate()),
                None => None,
            },
        }
    } else {
        Compilation::Error { stderr: response.stderr.concatenate() }
    })
}

async fn save_to_shortlink(
    http: &reqwest::Client,
    code: &str,
    compilerid: &str,
    language: &str,
    flags: &str,
    run_llvm_mca: bool,
) -> Result<String, Error> {
    #[derive(serde::Deserialize)]
    struct GodboltShortenerResponse {
        url: String,
    }

    let tools = if run_llvm_mca {
        serde_json::json! {
            [{"id": LLVM_MCA_TOOL_ID}]
        }
    } else {
        serde_json::json! {
            []
        }
    };

    let libraries = if language == "c++" {
        serde_json::json! {[
            {"id": "range-v3", "version": "trunk"},
            {"id": "fmt", "version": "trunk"}
        ]}
    } else {
        serde_json::json! {
            []
        }
    };

    let response = http
        .post("https://godbolt.org/api/shortener")
        .json(&serde_json::json! { {
            "sessions": [{
                "language": language,
                "source": code,
                "compilers": [{
                    "id": compilerid,
                    "options": flags,
                    "tools": tools,
                    "libs": libraries,
                }],
            }]
        } })
        .send()
        .await?;

    Ok(response.json::<GodboltShortenerResponse>().await?.url)
}

#[derive(PartialEq, Clone, Copy)]
enum GodboltMode {
    Asm,
    LlvmIr,
    Mca,
}

async fn generic_godbolt(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code: poise::CodeBlock,
    mode: GodboltMode,
) -> Result<(), Error> {
    let run_llvm_mca = mode == GodboltMode::Mca;

    let language = params.get("language").unwrap_or("rust");
    let (compiler, flags) = compiler_id_and_flags(ctx.data(), &params, language, mode).await?;

    let (lang, text);
    let mut note = String::new();

    let godbolt_result =
        compile_source(&ctx.data().http, &code.code, &compiler, &flags, language, run_llvm_mca)
            .await?;

    match godbolt_result {
        Compilation::Success { asm, stderr, stdout: _, llvm_mca } => {
            lang = match mode {
                GodboltMode::Asm => "x86asm",
                GodboltMode::Mca => "rust",
                GodboltMode::LlvmIr => "llvm",
            };
            text = match mode {
                GodboltMode::Mca => {
                    let llvm_mca = llvm_mca
                        .ok_or(anyhow::anyhow!("No llvm-mca result was sent by Godbolt"))?;
                    strip_llvm_mca_result(&llvm_mca).to_owned()
                },
                GodboltMode::Asm | GodboltMode::LlvmIr => asm,
            };
            if !stderr.is_empty() {
                note += "Note: compilation produced warnings\n";
            }
        },
        Compilation::Error { stderr } => {
            lang = language;
            text = stderr;
        },
    };

    if language == "rust" && !code.code.contains("pub fn") {
        note += "Note: only public functions (`pub fn`) are shown\n";
    }

    if text.trim().is_empty() {
        ctx.say(format!("``` ```{}", note)).await?;
    } else {
        crate::helpers::reply_potentially_long_text(
            ctx,
            &format!("```{}\n{}", lang, text),
            &format!("\n```{}", note),
            async {
                format!(
                    "Output too large. Godbolt link: <{}>",
                    save_to_shortlink(
                        &ctx.data().http,
                        &code.code,
                        &compiler,
                        language,
                        &flags,
                        run_llvm_mca
                    )
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("failed to generate godbolt shortlink: {}", e);
                        "failed to retrieve".to_owned()
                    }),
                )
            },
        )
        .await?;
    }

    Ok(())
}

/// View C++ code output using Godbolt
///
/// Compile C++ code using <https://godbolt.org>. Full optimizations are applied unless \
/// overriden.
/// ```
/// ?play_cpp flags={} compiler={} ``​`
/// int main() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `flags`: flags to pass to compiler invocation. Defaults to `"-Copt-level=3 --edition=2021"`
/// - `compiler`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`,
///   `beta` or full version like `1.45.2`
#[poise::command(prefix_command, broadcast_typing, track_edits, category = "Playground")]
pub async fn play_cpp(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code: poise::CodeBlock,
) -> Result<(), Error> {
    let language = "c++";
    let (compiler, flags) =
        compiler_id_and_flags(ctx.data(), &params, language, GodboltMode::Asm).await?;

    let (lang, text);
    let mut note = String::new();

    let godbolt_result = run_cpp_source(&ctx.data().http, &code.code, &compiler, &flags).await?;

    match godbolt_result {
        Compilation::Success { asm: _, stderr, stdout, llvm_mca: _ } => {
            lang = language;
            text = stdout;
            if !stderr.is_empty() {
                note += "Note: compilation produced warnings\n";
            }
        },
        Compilation::Error { stderr } => {
            lang = language;
            text = stderr;
        },
    };

    if text.trim().is_empty() {
        ctx.say(format!("``` ```{}", note)).await?;
    } else {
        crate::helpers::reply_potentially_long_text(
            ctx,
            &format!("```{}\n{}", lang, text),
            &format!("\n```{}", note),
            async {
                format!(
                    "Output too large. Godbolt link: <{}>",
                    save_to_shortlink(
                        &ctx.data().http,
                        &code.code,
                        &compiler,
                        language,
                        &flags,
                        false
                    )
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("failed to generate godbolt shortlink: {}", e);
                        "failed to retrieve".to_owned()
                    }),
                )
            },
        )
        .await?;
    }

    Ok(())
}

/// View assembly using Godbolt
///
/// Compile Rust code using <https://godbolt.org>. Full optimizations are applied unless \
/// overriden.
/// ```
/// ?godbolt language={} flags={} compiler={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `language`: language to use. Defaults to `rust`. Possible values: `rust`, `c++`
/// - `flags`: flags to pass to compiler invocation. Defaults to `"-Copt-level=3 --edition=2021"`
/// - `compiler`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`,
///   `beta` or full version like `1.45.2`
#[poise::command(prefix_command, broadcast_typing, track_edits, category = "Godbolt")]
pub async fn godbolt(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code: poise::CodeBlock,
) -> Result<(), Error> {
    generic_godbolt(ctx, params, code, GodboltMode::Asm).await
}

fn strip_llvm_mca_result(text: &str) -> &str {
    text[..text.find("Instruction Info").unwrap_or(text.len())].trim()
}

/// Run performance analysis using llvm-mca
///
/// Run the performance analysis tool llvm-mca using <https://godbolt.org>. Full optimizations \
/// are applied unless overriden.
/// ```
/// ?mca language={} flags={} compiler={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `language`: language to use. Defaults to `rust`. Possible values: `rust`, `c++`
/// - `flags`: flags to pass to compiler invocation. Defaults to `"-Copt-level=3 --edition=2021"`
/// - `compiler`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`,
///   `beta` or full version like `1.45.2`
#[poise::command(prefix_command, broadcast_typing, track_edits, category = "Godbolt")]
pub async fn mca(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code: poise::CodeBlock,
) -> Result<(), Error> {
    generic_godbolt(ctx, params, code, GodboltMode::Mca).await
}

/// View LLVM IR using Godbolt
///
/// Compile Rust code using <https://godbolt.org> and emits LLVM IR. Full optimizations \
/// are applied unless overriden.
///
/// Equivalent to ?godbolt but with extra flags `--emit=llvm-ir -Cdebuginfo=0`.
/// ```
/// ?llvmir language={} flags={} compiler={} ``​`
/// pub fn your_function() {
///     // Code
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `language`: language to use. Defaults to `rust`. Possible values: `rust`, `c++`
/// - `flags`: flags to pass to compiler invocation. Defaults to `"-Copt-level=3 --edition=2021"`
/// - `compiler`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`,
///   `beta` or full version like `1.45.2`
#[poise::command(prefix_command, broadcast_typing, track_edits, category = "Godbolt")]
pub async fn llvmir(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code: poise::CodeBlock,
) -> Result<(), Error> {
    generic_godbolt(ctx, params, code, GodboltMode::LlvmIr).await
}

// TODO: adjust doc
/// View difference between assembled functions
///
/// Compiles two Rust code snippets using <https://godbolt.org> and diffs them. Full optimizations \
/// are applied unless overriden.
/// ```
/// ?asmdiff language={} flags={} compiler={} ``​`
/// pub fn foo(x: u32) -> u32 {
///     x
/// }
/// ``​` ``​`
/// pub fn foo(x: u64) -> u64 {
///     x
/// }
/// ``​`
/// ```
/// Optional arguments:
/// - `language`: language to use. Defaults to `rust`. Possible values: `rust`, `c++`
/// - `flags`: flags to pass to compiler invocation. Defaults to `"-Copt-level=3 --edition=2021"`
/// - `compiler`: compiler version to invoke. Defaults to `nightly`. Possible values: `nightly`,
///   `beta` or full version like `1.45.2`
#[poise::command(prefix_command, broadcast_typing, track_edits, hide_in_help, category = "Godbolt")]
pub async fn asmdiff(
    ctx: Context<'_>,
    params: poise::KeyValueArgs,
    code1: poise::CodeBlock,
    code2: poise::CodeBlock,
) -> Result<(), Error> {
    let language = params.get("language").unwrap_or("rust");
    let (compiler, flags) =
        compiler_id_and_flags(ctx.data(), &params, language, GodboltMode::Asm).await?;

    let (asm1, asm2) = tokio::try_join!(
        compile_source(&ctx.data().http, &code1.code, &compiler, &flags, language, false),
        compile_source(&ctx.data().http, &code2.code, &compiler, &flags, language, false),
    )?;
    let result = match (asm1, asm2) {
        (Compilation::Success { asm: a, .. }, Compilation::Success { asm: b, .. }) => Ok((a, b)),
        (Compilation::Error { stderr }, _) => Err(stderr),
        (_, Compilation::Error { stderr }) => Err(stderr),
    };

    match result {
        Ok((asm1, asm2)) => {
            let mut path1 = std::env::temp_dir();
            path1.push("a");
            tokio::fs::write(&path1, asm1).await?;

            let mut path2 = std::env::temp_dir();
            path2.push("b");
            tokio::fs::write(&path2, asm2).await?;

            let diff = tokio::process::Command::new("git")
                .args(&["diff", "--no-index"])
                .arg(&path1)
                .arg(&path2)
                .output()
                .await?
                .stdout;

            crate::helpers::reply_potentially_long_text(
                ctx,
                &format!("```diff\n{}", String::from_utf8_lossy(&diff)),
                "```",
                async { String::from("(output was truncated)") },
            )
            .await?;
        },
        Err(stderr) => {
            crate::helpers::reply_potentially_long_text(
                ctx,
                &format!("```{}\n{}", language, stderr),
                "```",
                async { String::from("(output was truncated)") },
            )
            .await?;
        },
    }

    Ok(())
}
