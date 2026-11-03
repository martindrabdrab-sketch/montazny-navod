//! Montážny návod plugin for Open CAD Studio.
//!
//! Initial module: selection / visibility productivity tools.

use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{BuiltinPlugin, CommandRequest, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{
    CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef,
};

static MANIFEST: PluginManifest = PluginManifest {
    id: "sk.montazny.navod",
    name: "Montážny návod",
    version: env!("CARGO_PKG_VERSION"),
    description: "Plugin Montážny návod pre Open CAD Studio.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 20,
    xdata_apps: &[],
    command_prefixes: &[
        "MONTAZ_HIDE_SELECTED",
        "MONTAZ_ISOLATE_SELECTED",
        "MONTAZ_SHOW_ALL",
        "MONTAZ_SELECTION_INFO",
    ],
};

#[derive(Default)]
struct MontaznyNavodPlugin;

impl BuiltinPlugin for MontaznyNavodPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(MontaznyNavodModule)
    }

    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        match cmd {
            "MONTAZ_HIDE_SELECTED" => {
                run_selection_command(host, "HIDEOBJECTS", "Skryť označené");
                true
            }
            "MONTAZ_ISOLATE_SELECTED" => {
                run_selection_command(host, "ISOLATEOBJECTS", "Izolovať označené");
                true
            }
            "MONTAZ_SHOW_ALL" => {
                run_host_command(host, "UNISOLATEOBJECTS", "Zobraziť všetko");
                true
            }
            "MONTAZ_SELECTION_INFO" => {
                let count = host.selection().len();
                host.push_info(&format!("Montážny návod: označené objekty = {count}"));
                true
            }
            _ => false,
        }
    }
}

fn run_selection_command(host: &mut dyn HostApi, command: &str, label: &str) {
    let count = host.selection().len();
    if count == 0 {
        host.push_error(&format!("Montážny návod — {label}: najprv označ aspoň jeden objekt."));
        return;
    }

    run_host_command(host, command, label);
}

fn run_host_command(host: &mut dyn HostApi, command: &str, label: &str) {
    match host.run_command(CommandRequest::Run {
        line: command.to_string(),
    }) {
        Ok(outcome) => {
            if let Some(error) = outcome.error {
                host.push_error(&format!("Montážny návod — {label}: {error}"));
            } else if outcome.status == "completed" {
                host.push_info(&format!("Montážny návod — {label}: hotovo."));
            } else {
                host.push_info(&format!(
                    "Montážny návod — {label}: stav={}, čaká na {}.",
                    outcome.status,
                    outcome.blocked_by.unwrap_or_else(|| "vstup".to_string())
                ));
            }
        }
        Err(error) => {
            host.push_error(&format!("Montážny návod — {label}: {error}"));
        }
    }
}

struct MontaznyNavodModule;

impl CadModule for MontaznyNavodModule {
    fn id(&self) -> &'static str {
        MANIFEST.id
    }

    fn title(&self) -> &'static str {
        "Montážny návod"
    }

    fn ribbon_groups(&self) -> &[RibbonGroup] {
        static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();

        GROUPS.get_or_init(|| {
            vec![RibbonGroup {
                title: "Objekty",
                tools: vec![
                    RibbonItem::LargeTool(ToolDef {
                        id: "MONTAZ_ISOLATE_SELECTED",
                        label: "Izolovať",
                        icon: IconKind::Glyph("I"),
                        event: ModuleEvent::Command("MONTAZ_ISOLATE_SELECTED".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "MONTAZ_HIDE_SELECTED",
                        label: "Skryť",
                        icon: IconKind::Glyph("H"),
                        event: ModuleEvent::Command("MONTAZ_HIDE_SELECTED".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "MONTAZ_SHOW_ALL",
                        label: "Zobraziť všetko",
                        icon: IconKind::Glyph("A"),
                        event: ModuleEvent::Command("MONTAZ_SHOW_ALL".to_string()),
                    }),
                    RibbonItem::LargeTool(ToolDef {
                        id: "MONTAZ_SELECTION_INFO",
                        label: "Výber",
                        icon: IconKind::Glyph("#"),
                        event: ModuleEvent::Command("MONTAZ_SELECTION_INFO".to_string()),
                    }),
                ],
            }]
        })
    }
}

export_plugin!(MontaznyNavodPlugin);
