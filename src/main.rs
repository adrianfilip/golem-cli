// Copyright 2024 Golem Cloud
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

extern crate derive_more;

use std::fmt::Debug;

use clap::{Parser, Subcommand};
use clap_verbosity_flag::{Level, Verbosity};
use golem_cli::model::*;
use golem_client::Context;
use golem_examples::model::{ExampleName, GuestLanguage, GuestLanguageTier, PackageName};
use reqwest::Url;
use tracing_subscriber::FmtSubscriber;

use golem_cli::clients::template::TemplateClientLive;
use golem_cli::clients::worker::WorkerClientLive;
use golem_cli::examples;
use golem_cli::template::{TemplateHandler, TemplateHandlerLive, TemplateSubcommand};
use golem_cli::worker::{WorkerHandler, WorkerHandlerLive, WorkerSubcommand};

#[derive(Subcommand, Debug)]
#[command()]
enum Command {
    #[command()]
    Template {
        #[command(subcommand)]
        subcommand: TemplateSubcommand,
    },
    #[command()]
    Worker {
        #[command(subcommand)]
        subcommand: WorkerSubcommand,
    },
    #[command()]
    New {
        #[arg(short, long)]
        example: ExampleName,

        #[arg(short, long)]
        template_name: golem_examples::model::TemplateName,

        #[arg(short, long)]
        package_name: Option<PackageName>,
    },
    #[command()]
    ListExamples {
        #[arg(short, long)]
        min_tier: Option<GuestLanguageTier>,

        #[arg(short, long)]
        language: Option<GuestLanguage>,
    },
}

#[derive(Parser, Debug)]
#[command(author, version=env!("VERSION"), about, long_about, rename_all = "kebab-case")]
/// Command line interface for OSS version of Golem.
///
/// For Golem Cloud client see golem-cloud-cli instead: https://github.com/golemcloud/golem-cloud-cli
struct GolemCommand {
    #[command(flatten)]
    verbosity: Verbosity,

    #[arg(short = 'F', long, default_value = "yaml")]
    format: Format,

    #[arg(short = 'u', long)]
    /// Golem base url. Default: GOLEM_BASE_URL environment variable or http://localhost:9881.
    golem_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = GolemCommand::parse();

    if let Some(level) = command.verbosity.log_level() {
        let tracing_level = match level {
            Level::Error => tracing::Level::ERROR,
            Level::Warn => tracing::Level::WARN,
            Level::Info => tracing::Level::INFO,
            Level::Debug => tracing::Level::DEBUG,
            Level::Trace => tracing::Level::TRACE,
        };

        let subscriber = FmtSubscriber::builder()
            .with_max_level(tracing_level)
            .with_writer(std::io::stderr)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async_main(command))
}

async fn async_main(cmd: GolemCommand) -> Result<(), Box<dyn std::error::Error>> {
    let url_str = cmd
        .golem_url
        .or_else(|| std::env::var("GOLEM_BASE_URL").ok())
        .unwrap_or("http://localhost:9881".to_string());
    let url = Url::parse(&url_str).unwrap();
    let allow_insecure_str = std::env::var("GOLEM_ALLOW_INSECURE").unwrap_or("false".to_string());
    let allow_insecure = allow_insecure_str != "false";

    let mut builder = reqwest::Client::builder();
    if allow_insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }
    let client = builder.connection_verbose(true).build()?;

    let context = Context {
        base_url: url.clone(),
        client: client.clone(),
    };

    let template_client = TemplateClientLive {
        client: golem_client::api::TemplateClientLive {
            context: context.clone(),
        },
    };
    let template_srv = TemplateHandlerLive {
        client: template_client,
    };
    let worker_client = WorkerClientLive {
        client: golem_client::api::WorkerClientLive {
            context: context.clone(),
        },
        context: context.clone(),
        allow_insecure,
    };
    let worker_srv = WorkerHandlerLive {
        client: worker_client,
        templates: &template_srv,
    };

    let res = match cmd.command {
        Command::Template { subcommand } => template_srv.handle(subcommand).await,
        Command::Worker { subcommand } => worker_srv.handle(subcommand).await,
        Command::New {
            example,
            package_name,
            template_name,
        } => examples::process_new(example, template_name, package_name),
        Command::ListExamples { min_tier, language } => {
            examples::process_list_examples(min_tier, language)
        }
    };

    match res {
        Ok(res) => match res {
            GolemResult::Ok(r) => {
                r.println(&cmd.format);

                Ok(())
            }
            GolemResult::Str(s) => {
                println!("{s}");

                Ok(())
            }
            GolemResult::Json(json) => match &cmd.format {
                Format::Json => Ok(println!("{}", serde_json::to_string_pretty(&json).unwrap())),
                Format::Yaml => Ok(println!("{}", serde_yaml::to_string(&json).unwrap())),
            },
        },
        Err(err) => Err(Box::new(err)),
    }
}
