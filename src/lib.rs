//! Montážny návod plugin for Open CAD Studio.
//!
//! Assembly-step authoring stored directly in DWG/DXF XDATA.

use std::sync::atomic::{AtomicI32, Ordering};

use acadrust::entities::EntityCommon;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{BuiltinPlugin, CommandRequest, HostApi};
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{
    CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef,
};

const XDATA_APP: &str = "MONTAZ_NAVOD";
static CURRENT_STEP: AtomicI32 = AtomicI32::new(1);

static MANIFEST: PluginManifest = PluginManifest {
    id: "sk.montazny.navod",
    name: "Montážny návod",
    version: env!("CARGO_PKG_VERSION"),
    description: "Tvorba montážnych krokov zo zostavy DWG/DXF a SVG podkladov pre Affinity.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 20,
    xdata_apps: &[XDATA_APP],
    command_prefixes: &["MONTAZ_"],
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
            "MONTAZ_NEW_STEP" => {
                let step = CURRENT_STEP.fetch_add(1, Ordering::SeqCst) + 1;
                host.push_info(&format!("Montážny návod: aktuálny krok = {step}"));
                true
            }
            "MONTAZ_PREV_STEP" => {
                let mut step = CURRENT_STEP.load(Ordering::SeqCst);
                if step > 1 {
                    step -= 1;
                    CURRENT_STEP.store(step, Ordering::SeqCst);
                }
                host.push_info(&format!("Montážny návod: aktuálny krok = {step}"));
                true
            }
            "MONTAZ_NEXT_STEP" => {
                let step = CURRENT_STEP.fetch_add(1, Ordering::SeqCst) + 1;
                host.push_info(&format!("Montážny návod: aktuálny krok = {step}"));
                true
            }
            "MONTAZ_STEP_INFO" => {
                let step = CURRENT_STEP.load(Ordering::SeqCst);
                let selected = host.selection().len();
                host.push_info(&format!(
                    "Montážny návod: krok {step}, označené objekty = {selected}"
                ));
                true
            }
            "MONTAZ_ASSIGN_SELECTED" => {
                assign_selected_to_current_step(host);
                true
            }
            "MONTAZ_SHOW_STEP" => {
                show_step(host, true);
                true
            }
            "MONTAZ_SHOW_ONLY_NEW" => {
                show_step(host, false);
                true
            }
            "MONTAZ_EXPORT_SVG" => {
                export_svg(host);
                true
            }
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

fn assign_selected_to_current_step(host: &mut dyn HostApi) {
    let handles = host.selection();
    if handles.is_empty() {
        host.push_error("Montážny návod: najprv označ diely, ktoré patria do kroku.");
        return;
    }

    let step = CURRENT_STEP.load(Ordering::SeqCst);
    let mut ok = 0usize;
    let mut failed = 0usize;

    for handle in handles {
        let record = ExtendedDataRecord {
            application_name: XDATA_APP.to_string(),
            values: vec![
                XDataValue::Integer32(step),
                XDataValue::String(format!("KROK_{step:02}")),
            ],
        };
        match host.write_record(handle, record) {
            Ok(()) => ok += 1,
            Err(_) => failed += 1,
        }
    }

    host.push_info(&format!(
        "Montážny návod: krok {step} — priradené diely: {ok}, chyby: {failed}"
    ));
}

fn entity_step(host: &dyn HostApi, handle: acadrust::Handle) -> Option<i32> {
    let record = host.read_record(handle, XDATA_APP)?;
    record.values.iter().find_map(|value| match value {
        XDataValue::Integer32(step) => Some(*step),
        XDataValue::Integer16(step) => Some(i32::from(*step)),
        _ => None,
    })
}

fn assembly_handles(host: &mut dyn HostApi, cumulative: bool) -> Result<Vec<acadrust::Handle>, String> {
    let step = CURRENT_STEP.load(Ordering::SeqCst);
    let mut offset = 0usize;
    let mut handles = Vec::new();

    loop {
        let query = format!(
            r#"{{"xdata_app":"{XDATA_APP}","offset":{offset},"limit":1000}}"#
        );
        let page = host.query_entities(&query)?;
        if page.entities.is_empty() {
            break;
        }

        for entity in page.entities {
            let handle = entity.as_entity().handle();
            if !handle.is_valid() {
                continue;
            }
            if let Some(entity_step) = entity_step(host, handle) {
                let include = if cumulative {
                    entity_step <= step
                } else {
                    entity_step == step
                };
                if include {
                    handles.push(handle);
                }
            }
        }

        match page.next_offset {
            Some(next) => offset = next as usize,
            None => break,
        }
    }

    Ok(handles)
}

fn show_step(host: &mut dyn HostApi, cumulative: bool) {
    let step = CURRENT_STEP.load(Ordering::SeqCst);
    let handles = match assembly_handles(host, cumulative) {
        Ok(handles) => handles,
        Err(error) => {
            host.push_error(&format!("Montážny návod: načítanie krokov zlyhalo: {error}"));
            return;
        }
    };

    if handles.is_empty() {
        host.push_error(&format!(
            "Montážny návod: pre krok {step} neboli nájdené žiadne priradené diely."
        ));
        return;
    }

    if let Err(error) = host.set_selection(&handles) {
        host.push_error(&format!("Montážny návod: označenie dielov zlyhalo: {error}"));
        return;
    }

    run_host_command(host, "ISOLATEOBJECTS", "Zobraziť montážny krok");
    host.push_info(&format!(
        "Montážny návod: krok {step} — zobrazených {} dielov ({})",
        handles.len(),
        if cumulative { "kumulatívne" } else { "iba nové" }
    ));
}

fn export_svg(host: &mut dyn HostApi) {
    let step = CURRENT_STEP.load(Ordering::SeqCst);
    host.push_info(&format!(
        "Montážny návod: export kroku {step}. Ulož SVG pod rovnakým názvom (napr. KROK_{step:02}.svg), aby Affinity obnovilo linked resource."
    ));
    run_host_command(host, "EXPORTSVG", "Export SVG pre Affinity");
}

fn run_selection_command(host: &mut dyn HostApi, command: &str, label: &str) {
    if host.selection().is_empty() {
        host.push_error(&format!(
            "Montážny návod — {label}: najprv označ aspoň jeden objekt."
        ));
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
            vec![
                RibbonGroup {
                    title: "Kroky",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_ASSIGN_SELECTED",
                            label: "Priradiť\ndiely",
                            icon: IconKind::Glyph("+"),
                            event: ModuleEvent::Command("MONTAZ_ASSIGN_SELECTED".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_PREV_STEP",
                            label: "Predch.\nkrok",
                            icon: IconKind::Glyph("<"),
                            event: ModuleEvent::Command("MONTAZ_PREV_STEP".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_NEXT_STEP",
                            label: "Ďalší\nkrok",
                            icon: IconKind::Glyph(">"),
                            event: ModuleEvent::Command("MONTAZ_NEXT_STEP".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_STEP_INFO",
                            label: "Aktuálny\nkrok",
                            icon: IconKind::Glyph("#"),
                            event: ModuleEvent::Command("MONTAZ_STEP_INFO".to_string()),
                        }),
                    ],
                },
                RibbonGroup {
                    title: "Náhľad",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_SHOW_STEP",
                            label: "Zobraziť\nkrok",
                            icon: IconKind::Glyph("S"),
                            event: ModuleEvent::Command("MONTAZ_SHOW_STEP".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_SHOW_ONLY_NEW",
                            label: "Iba nové\ndiely",
                            icon: IconKind::Glyph("N"),
                            event: ModuleEvent::Command("MONTAZ_SHOW_ONLY_NEW".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_SHOW_ALL",
                            label: "Zobraziť\nvšetko",
                            icon: IconKind::Glyph("A"),
                            event: ModuleEvent::Command("MONTAZ_SHOW_ALL".to_string()),
                        }),
                    ],
                },
                RibbonGroup {
                    title: "Affinity",
                    tools: vec![RibbonItem::LargeTool(ToolDef {
                        id: "MONTAZ_EXPORT_SVG",
                        label: "Export\nSVG",
                        icon: IconKind::Glyph("SVG"),
                        event: ModuleEvent::Command("MONTAZ_EXPORT_SVG".to_string()),
                    })],
                },
            ]
        })
    }
}

export_plugin!(MontaznyNavodPlugin);
