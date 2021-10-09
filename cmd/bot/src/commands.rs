use stores::config::{ConfigStore, CreateScript, Script, ScriptContext};
use tracing::{error, info, instrument};
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::Cluster;
use twilight_model::{
    gateway::payload::MessageCreate,
    guild::Permissions,
    id::{ChannelId, RoleId},
};
use twilight_util::permission_calculator::PermissionCalculator;

use crate::{vm_manager, BotContext};

#[derive(Clone)]
pub struct CommandContext<CT> {
    pub(crate) http: twilight_http::Client,
    pub(crate) cluster: Cluster,
    pub(crate) state: InMemoryCache,
    pub(crate) config_store: CT,
    pub(crate) vm_manager: vm_manager::Manager<CT>,
}

#[derive(Debug)]
pub enum Command {
    AddScript(String),
    UpdateScript(String),
    GetScript(String),
    DeleteScript(String),
    ListScripts,

    AttachScript(String, ScriptContext),
    DetachScript(String, ScriptContext),
    ListScriptAttachments(Option<String>),
    DetachAllScript(String),

    StartVM,
    SetErrorChannel(bool),
}

#[derive(Debug)]
pub struct ParsedCommand {
    m: MessageCreate,
    command: Command,
}

pub(crate) fn check_for_command<CT>(
    ctx: &BotContext<CT>,
    m: MessageCreate,
) -> Option<ParsedCommand> {
    if let Some(mem) = &m.member {
        let roles: Vec<_> = mem
            .roles
            .iter()
            .map(|id| {
                (
                    *id,
                    ctx.state
                        .role(*id)
                        .map(|p| p.permissions)
                        .unwrap_or_else(Permissions::empty),
                )
            })
            .collect();

        let everyone = ctx
            .state
            .role(RoleId(m.guild_id.unwrap().0))
            .map(|everyone_role| everyone_role.permissions)
            .unwrap_or_else(Permissions::empty);

        // if let Some(everyone_role) = ctx.state.role(RoleId(m.guild_id.unwrap().0)) {
        //     roles.push((everyone_role.id, everyone_role.permissions));
        // }

        let calculator = PermissionCalculator::new(m.guild_id?, m.author.id, everyone, &roles);
        if !(calculator.root().intersects(Permissions::MANAGE_GUILD)) {
            return None;
        }
    } else {
        return None;
    }

    let mut split = m.content.split(' ');
    if let Some(prefix) = split.next() {
        if prefix == "!jack" {
            let collected: Vec<String> = split.map(|e| e.to_string()).collect();
            match parse_command(&m, collected) {
                Ok(Some(cmd)) => {
                    info!("Parsed command: {:?}", cmd);
                    return Some(ParsedCommand { m, command: cmd });
                }
                Err(e) => {
                    info!("failed parsing command: {}", e);
                }
                _ => {}
            }
        }
    }

    None
}

fn parse_command(_m: &MessageCreate, split: Vec<String>) -> Result<Option<Command>, String> {
    let mut iter = split.into_iter();
    let prefix = if let Some(prefix) = iter.next() {
        prefix
    } else {
        return Ok(None);
    };

    match prefix.as_str() {
        "script" => {
            if let Some(sub) = iter.next() {
                match sub.as_str().to_lowercase().as_str() {
                    "add" => Ok(Some(Command::AddScript(cmd_parse_string_rest(&mut iter)?))),
                    "del" => Ok(Some(Command::DeleteScript(cmd_parse_string_word(
                        &mut iter,
                    )?))),
                    "get" => Ok(Some(Command::GetScript(cmd_parse_string_word(&mut iter)?))),
                    "update" => Ok(Some(Command::UpdateScript(cmd_parse_string_rest(
                        &mut iter,
                    )?))),
                    "list" => Ok(Some(Command::ListScripts)),

                    "linkchannel" => Ok(Some(Command::AttachScript(
                        cmd_parse_string_word(&mut iter)?,
                        ScriptContext::Channel(ChannelId(cmd_parse_uint64(&mut iter)?)),
                    ))),
                    "unlinkchannel" => Ok(Some(Command::DetachScript(
                        cmd_parse_string_word(&mut iter)?,
                        ScriptContext::Channel(ChannelId(cmd_parse_uint64(&mut iter)?)),
                    ))),
                    "linkguild" => Ok(Some(Command::AttachScript(
                        cmd_parse_string_word(&mut iter)?,
                        ScriptContext::Guild,
                    ))),
                    "unlinkguild" => Ok(Some(Command::DetachScript(
                        cmd_parse_string_word(&mut iter)?,
                        ScriptContext::Guild,
                    ))),
                    "listlinks" => Ok(Some(Command::ListScriptAttachments(
                        cmd_parse_string_word(&mut iter).ok(),
                    ))),
                    "unlinkall" => Ok(Some(Command::DetachAllScript(cmd_parse_string_word(
                        &mut iter,
                    )?))),
                    _ => Ok(None),
                }
            } else {
                Ok(None)
            }
        }
        "seterrorchannel" => Ok(Some(Command::SetErrorChannel(true))),
        "unseterrorchannel" => Ok(Some(Command::SetErrorChannel(false))),
        "startvm" => Ok(Some(Command::StartVM)),
        _ => Ok(None),
    }
}

fn cmd_parse_uint64<T: Iterator<Item = String>>(iter: &mut T) -> Result<u64, String> {
    match iter.next() {
        None => Err("no more args".to_string()),
        Some(s) => Ok(s.parse().map_err(|e| format!("failed parsing: {}", e))?),
    }
}

fn cmd_parse_string_rest<T: Iterator<Item = String>>(iter: &mut T) -> Result<String, String> {
    let mut rest = iter.collect::<Vec<_>>().join(" ");

    // strip codeblock
    if rest.starts_with("```") {
        // strip first line
        let mut lines = rest.split('\n');
        lines.next();

        rest = lines.collect::<Vec<_>>().join("\n");

        // strip end codeblock
        rest = rest.trim_end_matches("```").to_string();
    }

    Ok(rest)
}

fn cmd_parse_string_word<T: Iterator<Item = String>>(iter: &mut T) -> Result<String, String> {
    match iter.next() {
        None => Err("no more args".to_string()),
        Some(s) => Ok(s),
    }
}

#[instrument(skip(ctx, cmd))]
pub(crate) async fn handle_command<CT: ConfigStore + Send + Sync + 'static>(
    ctx: CommandContext<CT>,
    cmd: ParsedCommand,
) {
    match run_command(&ctx, &cmd).await {
        Ok(Some(s)) => {
            match ctx
                .http
                .create_message(cmd.m.channel_id)
                .content(&s)
                .unwrap()
                .exec()
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!(err = %e, "failed sending command response")
                }
            }
        }
        Ok(None) => {}
        Err(e) => {
            error!("Failed running command :(: {}", e);

            match ctx
                .http
                .create_message(cmd.m.channel_id)
                .content("Failed handling command :(")
                .unwrap()
                .exec()
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    error!(err = %e, "failed sending command response")
                }
            }
        }
    }
}

#[instrument(skip(ctx, cmd))]
async fn run_command<CT: ConfigStore + Send + Sync + 'static>(
    ctx: &CommandContext<CT>,
    cmd: &ParsedCommand,
) -> Result<Option<String>, String> {
    match &cmd.command {
        Command::AddScript(source) | Command::UpdateScript(source) => {
            let compiled = tscompiler::compile_typescript(source)
                .map_err(|e| format!("failed compiling: {:?}", e))?;

            println!("Compiled: {}", compiled);

            let header = match vm::validate_script(
                compiled.clone(),
                runtime::jsmodules::create_module_map(),
            )
            .await
            {
                Ok(h) => h,
                Err(e) => {
                    return Ok(Some(format!("failed validating script: {}", e)));
                }
            };

            match ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), header.name.clone())
                .await
            {
                Ok(existing) => {
                    let script = ctx
                        .config_store
                        .update_script(
                            cmd.m.guild_id.unwrap(),
                            Script {
                                original_source: source.clone(),
                                compiled_js: compiled,
                                ..existing
                            },
                        )
                        .await
                        .map_err(|e| format!("failed updating script :( {}", e))?;

                    ctx.vm_manager
                        .update_script(cmd.m.guild_id.unwrap(), script)
                        .await?;

                    Ok(Some(format!("Script {} has been updated!", header.name)))
                }

                Err(_) => {
                    ctx.config_store
                        .create_script(
                            cmd.m.guild_id.unwrap(),
                            CreateScript {
                                name: header.name.clone(),
                                original_source: source.clone(),
                                compiled_js: compiled,
                            },
                        )
                        .await
                        .map_err(|e| format!("failed creating script :( {}", e))?;

                    Ok(Some(format!(
                        "Script {} has been added! (note that it still needs to be linked to a \
                         context)",
                        header.name
                    )))
                }
            }
        }
        Command::DeleteScript(name) => {
            let script = ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("unknown script: {}", e))?;

            ctx.vm_manager
                .detach_all_script(cmd.m.guild_id.unwrap(), script.id)
                .await
                .ok();

            ctx.config_store
                .del_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("failed deleting script: {}", e))?;

            Ok(Some(format!("Script {} has been deleted!", name)))
        }
        Command::GetScript(name) => {
            let script = ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("unknown script: {}", e))?;

            Ok(Some(format!(
                "Script {}:\nOriginal: ```ts\n{}\n```\nCompiled: ```js\n{}\n```",
                script.name, script.original_source, script.compiled_js
            )))
        }
        Command::AttachScript(name, context) => {
            let script = ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("unknown script: {}", e))?;

            ctx.config_store
                .link_script(cmd.m.guild_id.unwrap(), name.clone(), context.clone())
                .await
                .map_err(|e| format!("failed linking script: {}", e))?;

            ctx.vm_manager
                .attach_script(cmd.m.guild_id.unwrap(), script, context.clone())
                .await
                .unwrap();

            Ok(Some(format!(
                "Script {} has been attatched to {:?}!",
                name, context
            )))
        }
        Command::DetachScript(name, context) => {
            let script = ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("unknown script: {}", e))?;

            ctx.config_store
                .unlink_script(cmd.m.guild_id.unwrap(), name.clone(), context.clone())
                .await
                .map_err(|e| format!("failed adding script: {}", e))?;

            ctx.vm_manager
                .detach_scripts(cmd.m.guild_id.unwrap(), vec![(script.id, context.clone())])
                .await
                .unwrap();

            Ok(Some(format!(
                "Script {} has been detatched from {:?}!",
                name, context
            )))
        }
        Command::DetachAllScript(name) => {
            let script = ctx
                .config_store
                .get_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("unknown script: {}", e))?;

            let all = ctx
                .config_store
                .unlink_all_script(cmd.m.guild_id.unwrap(), name.clone())
                .await
                .map_err(|e| format!("failed adding script: {}", e))?;

            ctx.vm_manager
                .detach_all_script(cmd.m.guild_id.unwrap(), script.id)
                .await
                .unwrap();

            Ok(Some(format!(
                "Script {} has been detached from {} contexts!",
                name, all
            )))
        }
        Command::ListScripts => {
            let scripts = ctx
                .config_store
                .list_scripts(cmd.m.guild_id.unwrap())
                .await
                .map_err(|e| format!("failed fetching scripts: {}", e))?;

            let links = ctx
                .config_store
                .list_links(cmd.m.guild_id.unwrap())
                .await
                .map_err(|e| format!("failed fetching links: {}", e))?;

            let summary = scripts
                .into_iter()
                .map(|e| (links.iter().filter(|l| l.script_name == e.name).count(), e))
                .map(|(num_links, script)| {
                    format!("{} linked to {} contexts\n", script.name, num_links)
                })
                .collect::<String>();

            Ok(Some(format!(
                "Scripts on this guild: ```\n{}\n```",
                summary
            )))
        }
        Command::ListScriptAttachments(_) => {
            // let _scripts = ctx
            //     .config_store
            //     .list_scripts(cmd.m.guild_id.unwrap())
            //     .await
            //     .map_err(|e| format!("failed fetching scripts: {}", e))?;

            let links = ctx
                .config_store
                .list_links(cmd.m.guild_id.unwrap())
                .await
                .map_err(|e| format!("failed fetching links: {}", e))?;

            let summary = links
                .into_iter()
                .map(|l| format!("script {} linked to {:?}\n", l.script_name, l.context))
                .collect::<String>();

            Ok(Some(format!(
                "Script links on this guild: ```\n{}\n```",
                summary
            )))
        }
        Command::StartVM => {
            ctx.vm_manager
                .restart_guild_vm(cmd.m.guild_id.unwrap())
                .await
                .map_err(|e| format!("failed restarting guild vm: {}", e))?;

            Ok(Some(
                "Restarting your guild's vm... (note that if it keeps stopping, there might be a \
                 runaway script that contains something like a infinite loop, you should find \
                 and remove the culprit)"
                    .to_string(),
            ))
        }
        Command::SetErrorChannel(set) => {
            let mut conf = ctx
                .config_store
                .get_guild_meta_config_or_default(cmd.m.guild_id.unwrap())
                .await
                .map_err(|e| format!("failed fetching your guild config: {}", e))?;

            if *set {
                conf.error_channel_id = Some(cmd.m.channel_id);
            } else {
                conf.error_channel_id = None;
            }

            ctx.config_store
                .update_guild_meta_config(&conf)
                .await
                .map_err(|e| format!("failed updating the config: {}", e))?;

            Ok(Some(if *set {
                "set the error channel to this channel".to_string()
            } else {
                "unset the error channel, you will no longer receive notifications anywhere about \
                 errors"
                    .to_string()
            }))
        }
    }
}
