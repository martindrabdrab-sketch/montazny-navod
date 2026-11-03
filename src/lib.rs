//! Montážny návod plugin for Open CAD Studio.
//!
//! v0.4: assembly browser, DWG/XDATA metadata, working copies, BOM table,
//! montage steps and Affinity SVG workflow.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use ocs_plugin_api::dialog::{DialogControl, DialogValue, PanelDef};
use ocs_plugin_api::export_plugin;
use ocs_plugin_api::host::{
    acadrust as codec, BuiltinPlugin, CommandRequest, EntityType, ExtendedDataRecord, Handle,
    HostApi, HostNotification, XDataValue,
};
use codec::entities::{EntityCommon, TableBuilder};
use codec::types::Vector3;
use ocs_plugin_api::manifest::{ApiVersion, PluginManifest};
use ocs_plugin_api::ribbon::{
    CadModule, IconKind, ModuleEvent, RibbonGroup, RibbonItem, ToolDef,
};

const XDATA_APP: &str = "MONTAZ_NAVOD";
const PANEL_ID: &str = "montazny_navod_panel";

static MANIFEST: PluginManifest = PluginManifest {
    id: "sk.montazny.navod",
    name: "Montážny návod",
    version: env!("CARGO_PKG_VERSION"),
    description: "Strom zostavy, montážne kroky, pracovné kópie, BOM a Affinity export.",
    api_version: ApiVersion::CURRENT,
    ribbon_order: 20,
    xdata_apps: &[XDATA_APP],
    command_prefixes: &["MONTAZ_"],
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ItemKind {
    Part,
    Hardware,
}

impl ItemKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Part => "PART",
            Self::Hardware => "HARDWARE",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Part => "Dielec",
            Self::Hardware => "Kovanie",
        }
    }
}

#[derive(Clone, Debug)]
struct Meta {
    cabinet: i32,
    kind: ItemKind,
    name: String,
    qty: i32,
    step: i32,
    role: String,
}

#[derive(Clone, Debug)]
struct ScanEntry {
    handle: Handle,
    cabinet: i32,
    kind: ItemKind,
    name: String,
    qty: i32,
    step: i32,
    role: String,
    layer: String,
    tagged: bool,
}

#[derive(Debug)]
struct PluginState {
    entries: Vec<ScanEntry>,
    cabinet: i32,
    kind: ItemKind,
    name: String,
    qty: i32,
    step: i32,
    bom_x: f64,
    bom_y: f64,
    bom_z: f64,
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            cabinet: 1,
            kind: ItemKind::Part,
            name: String::new(),
            qty: 1,
            step: 1,
            bom_x: 0.0,
            bom_y: 0.0,
            bom_z: 0.0,
        }
    }
}

#[derive(Default)]
struct MontaznyNavodPlugin {
    state: Mutex<PluginState>,
}

impl BuiltinPlugin for MontaznyNavodPlugin {
    fn manifest(&self) -> &'static PluginManifest {
        &MANIFEST
    }

    fn ribbon(&self) -> Box<dyn CadModule> {
        Box::new(MontaznyNavodModule)
    }

    fn on_load(&mut self, host: &mut dyn HostApi) {
        self.rebuild_panel(host, true);
    }

    fn dispatch(&self, host: &mut dyn HostApi, cmd: &str) -> bool {
        if let Some(index_text) = cmd.strip_prefix("MONTAZ_PICK_") {
            if let Ok(index) = index_text.parse::<usize>() {
                self.pick_tree_item(host, index);
            }
            return true;
        }

        match cmd {
            "MONTAZ_PANEL" => {
                host.toggle_panel(PANEL_ID, None);
                true
            }
            "MONTAZ_REFRESH" => {
                self.rebuild_panel(host, true);
                true
            }
            "MONTAZ_LOAD_SELECTED" => {
                self.load_selected_into_editor(host);
                true
            }
            "MONTAZ_TAG_SELECTED" => {
                self.tag_selected(host);
                true
            }
            "MONTAZ_UNTAG_SELECTED" => {
                self.untag_selected(host);
                true
            }
            "MONTAZ_WORK_COPY" => {
                self.make_working_copy(host);
                true
            }
            "MONTAZ_PREV_STEP" => {
                self.change_step(host, -1);
                true
            }
            "MONTAZ_NEXT_STEP" => {
                self.change_step(host, 1);
                true
            }
            "MONTAZ_ASSIGN_STEP" => {
                self.assign_step(host);
                true
            }
            "MONTAZ_SHOW_STEP" => {
                self.show_step(host, true);
                true
            }
            "MONTAZ_SHOW_ONLY_NEW" => {
                self.show_step(host, false);
                true
            }
            "MONTAZ_SHOW_ALL" => {
                run_host_command(host, "UNISOLATEOBJECTS", "Zobraziť všetko");
                true
            }
            "MONTAZ_BOM_INSERT" => {
                self.insert_bom_table(host);
                true
            }
            "MONTAZ_EXPORT_SVG" => {
                self.export_svg(host);
                true
            }
            "MONTAZ_SELECTION_INFO" => {
                host.push_info(&format!(
                    "Montážny návod: označené objekty = {}",
                    host.selection().len()
                ));
                true
            }
            _ => false,
        }
    }

    fn on_notification(&mut self, _command_id: Option<u64>, notification: HostNotification) {
        let HostNotification::PanelAction {
            panel_id,
            control_id,
            value,
            ..
        } = notification
        else {
            return;
        };
        if panel_id != PANEL_ID {
            return;
        }

        let Ok(mut state) = self.state.lock() else {
            return;
        };

        match (control_id.as_str(), value) {
            ("cabinet_no", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<i32>() {
                    state.cabinet = n.max(1);
                }
            }
            ("item_type", Some(DialogValue::Choice { index, .. })) => {
                state.kind = if index == 1 {
                    ItemKind::Hardware
                } else {
                    ItemKind::Part
                };
            }
            ("item_name", Some(DialogValue::Text(v))) => state.name = v,
            ("qty", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<i32>() {
                    state.qty = n.max(1);
                }
            }
            ("step_no", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<i32>() {
                    state.step = n.max(1);
                }
            }
            ("bom_x", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<f64>() {
                    state.bom_x = n;
                }
            }
            ("bom_y", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<f64>() {
                    state.bom_y = n;
                }
            }
            ("bom_z", Some(DialogValue::Text(v))) => {
                if let Ok(n) = v.trim().parse::<f64>() {
                    state.bom_z = n;
                }
            }
            _ => {}
        }
    }
}

impl MontaznyNavodPlugin {
    fn rebuild_panel(&self, host: &mut dyn HostApi, show: bool) {
        let entries = scan_drawing(host);
        let panel = build_panel(&entries, &self.state);

        if let Ok(mut state) = self.state.lock() {
            state.entries = entries;
        }

        if !host.register_panel(panel) {
            host.push_error("Montážny návod: nepodarilo sa zaregistrovať bočný panel.");
            return;
        }
        host.toggle_panel(PANEL_ID, Some(show));
    }

    fn pick_tree_item(&self, host: &mut dyn HostApi, index: usize) {
        let entry = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.entries.get(index).cloned());
        let Some(entry) = entry else {
            host.push_error("Montážny návod: položka stromu už neexistuje. Klikni Obnoviť.");
            return;
        };

        if let Err(error) = host.set_selection(&[entry.handle]) {
            host.push_error(&format!("Montážny návod: výber objektu zlyhal: {error}"));
            return;
        }

        if let Ok(mut state) = self.state.lock() {
            state.cabinet = entry.cabinet;
            state.kind = entry.kind;
            state.name = entry.name.clone();
            state.qty = entry.qty.max(1);
            state.step = entry.step.max(1);
        }
        update_editor_panel(host, &entry);

        host.push_info(&format!(
            "Montážny návod: vybrané {} — {} (skrinka {:02}).",
            entry.kind.label(),
            entry.name,
            entry.cabinet
        ));
    }

    fn load_selected_into_editor(&self, host: &mut dyn HostApi) {
        let handles = host.selection();
        let Some(&handle) = handles.first() else {
            host.push_error("Montážny návod: najprv označ objekt v DWG.");
            return;
        };

        let entry = scan_one(host, handle, 1);
        if let Ok(mut state) = self.state.lock() {
            state.cabinet = entry.cabinet;
            state.kind = entry.kind;
            state.name = entry.name.clone();
            state.qty = entry.qty.max(1);
            state.step = entry.step.max(1);
        }
        update_editor_panel(host, &entry);
    }

    fn tag_selected(&self, host: &mut dyn HostApi) {
        let handles = host.selection();
        if handles.is_empty() {
            host.push_error("Montážny návod: najprv označ dielce alebo kovanie.");
            return;
        }

        let (cabinet, kind, custom_name, qty, step) = match self.state.lock() {
            Ok(state) => (
                state.cabinet,
                state.kind,
                state.name.trim().to_string(),
                state.qty.max(1),
                state.step.max(1),
            ),
            Err(_) => return,
        };

        let inferred: HashMap<Handle, String> = handles
            .iter()
            .enumerate()
            .map(|(i, &h)| (h, scan_one(host, h, i + 1).name))
            .collect();

        host.begin_undo_group("MONTAZ: zaradenie objektov");
        let mut ok = 0usize;
        for handle in handles {
            let name = if custom_name.is_empty() {
                inferred
                    .get(&handle)
                    .cloned()
                    .unwrap_or_else(|| "Diel".to_string())
            } else {
                custom_name.clone()
            };
            let meta = Meta {
                cabinet,
                kind,
                name,
                qty,
                step,
                role: "MASTER".to_string(),
            };
            if host.write_record(handle, meta_record(&meta)) {
                ok += 1;
            }
        }
        host.end_undo_group();

        host.push_info(&format!("Montážny návod: zaradených objektov = {ok}."));
        self.rebuild_panel(host, true);
    }

    fn untag_selected(&self, host: &mut dyn HostApi) {
        let handles = host.selection();
        if handles.is_empty() {
            host.push_error("Montážny návod: najprv označ objekty.");
            return;
        }

        host.begin_undo_group("MONTAZ: odstránenie zaradenia");
        for handle in handles {
            let _ = host.remove_record(handle, XDATA_APP);
        }
        host.end_undo_group();
        self.rebuild_panel(host, true);
    }

    fn make_working_copy(&self, host: &mut dyn HostApi) {
        let handles = host.selection();
        if handles.is_empty() {
            host.push_error("Montážny návod: najprv označ dielce pre pracovnú kópiu.");
            return;
        }

        let step = self.state.lock().map(|s| s.step.max(1)).unwrap_or(1);
        let cloned_entities: Vec<(Handle, EntityType)> = {
            let doc = host.document();
            handles
                .iter()
                .filter_map(|&handle| doc.get_entity(handle).cloned().map(|entity| (handle, entity)))
                .collect()
        };

        let sources: Vec<(EntityType, Meta)> = cloned_entities
            .into_iter()
            .enumerate()
            .map(|(i, (handle, mut entity))| {
                entity.common_mut().handle = Handle::NULL;
                entity.common_mut().owner_handle = Handle::NULL;
                entity.common_mut().reactors.clear();
                entity.common_mut().xdictionary_handle = None;

                let src = scan_one(host, handle, i + 1);
                let meta = Meta {
                    cabinet: src.cabinet,
                    kind: src.kind,
                    name: src.name,
                    qty: src.qty.max(1),
                    step,
                    role: "WORK".to_string(),
                };
                (entity, meta)
            })
            .collect();

        if sources.is_empty() {
            host.push_error("Montážny návod: označené objekty sa nepodarilo skopírovať.");
            return;
        }

        host.begin_undo_group("MONTAZ: pracovná kópia");
        let mut new_handles = Vec::new();
        for (entity, meta) in sources {
            let handle = host.add_entity(entity);
            if !handle.is_null() {
                let _ = host.write_record(handle, meta_record(&meta));
                new_handles.push(handle);
            }
        }
        host.end_undo_group();

        if !new_handles.is_empty() {
            let _ = host.set_selection(&new_handles);
            host.push_info(&format!(
                "Montážny návod: vytvorených pracovných kópií = {}. Teraz ich môžeš MOVE/3DMOVE/ROTATE/3DROTATE.",
                new_handles.len()
            ));
        }
        self.rebuild_panel(host, true);
    }

    fn change_step(&self, host: &mut dyn HostApi, delta: i32) {
        let step = match self.state.lock() {
            Ok(mut state) => {
                state.step = (state.step + delta).max(1);
                state.step
            }
            Err(_) => 1,
        };

        let mut values = HashMap::new();
        values.insert("step_no".to_string(), DialogValue::Text(step.to_string()));
        host.update_panel(PANEL_ID, values);
        host.push_info(&format!("Montážny návod: aktuálny krok = {step}."));
    }

    fn assign_step(&self, host: &mut dyn HostApi) {
        let handles = host.selection();
        if handles.is_empty() {
            host.push_error("Montážny návod: najprv označ objekty.");
            return;
        }
        let step = self.state.lock().map(|s| s.step.max(1)).unwrap_or(1);

        host.begin_undo_group("MONTAZ: priradenie kroku");
        for (i, handle) in handles.into_iter().enumerate() {
            let entry = scan_one(host, handle, i + 1);
            let meta = Meta {
                cabinet: entry.cabinet,
                kind: entry.kind,
                name: entry.name,
                qty: entry.qty.max(1),
                step,
                role: entry.role,
            };
            let _ = host.write_record(handle, meta_record(&meta));
        }
        host.end_undo_group();
        self.rebuild_panel(host, true);
    }

    fn show_step(&self, host: &mut dyn HostApi, cumulative: bool) {
        let step = self.state.lock().map(|s| s.step.max(1)).unwrap_or(1);
        let entries = scan_drawing(host);
        let handles: Vec<Handle> = entries
            .iter()
            .filter(|entry| {
                entry.step > 0
                    && if cumulative {
                        entry.step <= step
                    } else {
                        entry.step == step
                    }
                    && entry.role != "BOM"
            })
            .map(|entry| entry.handle)
            .collect();

        if handles.is_empty() {
            host.push_error(&format!(
                "Montážny návod: krok {step} zatiaľ nemá priradené objekty."
            ));
            return;
        }

        if let Err(error) = host.set_selection(&handles) {
            host.push_error(&format!("Montážny návod: výber kroku zlyhal: {error}"));
            return;
        }
        run_host_command(host, "ISOLATEOBJECTS", "Izolovať montážny krok");
    }

    fn insert_bom_table(&self, host: &mut dyn HostApi) {
        let entries = scan_drawing(host);
        let mut bom: BTreeMap<(i32, ItemKind, String, i32), i32> = BTreeMap::new();

        for entry in entries {
            if entry.role == "WORK" || entry.role == "BOM" {
                continue;
            }
            let key = (
                entry.cabinet,
                entry.kind,
                entry.name.clone(),
                entry.step.max(0),
            );
            *bom.entry(key).or_insert(0) += entry.qty.max(1);
        }

        if bom.is_empty() {
            host.push_error("Montážny návod: v DWG neboli nájdené položky pre BOM.");
            return;
        }

        let (x, y, z) = self
            .state
            .lock()
            .map(|s| (s.bom_x, s.bom_y, s.bom_z))
            .unwrap_or((0.0, 0.0, 0.0));

        let rows = bom.len() + 1;
        let mut table = TableBuilder::new(rows, 6)
            .at(Vector3::new(x, y, z))
            .row_height(8.0)
            .column_width(35.0)
            .build();

        table.set_column_width(0, 12.0);
        table.set_column_width(1, 24.0);
        table.set_column_width(2, 55.0);
        table.set_column_width(3, 18.0);
        table.set_column_width(4, 20.0);
        table.set_column_width(5, 18.0);

        for (col, text) in ["Poz.", "Typ", "Názov", "Množstvo", "Skrinka", "Krok"]
            .into_iter()
            .enumerate()
        {
            table.set_cell_text(0, col, text);
        }

        for (row, ((cabinet, kind, name, step), qty)) in bom.into_iter().enumerate() {
            let r = row + 1;
            table.set_cell_text(r, 0, &(r).to_string());
            table.set_cell_text(r, 1, kind.label());
            table.set_cell_text(r, 2, &name);
            table.set_cell_text(r, 3, &qty.to_string());
            table.set_cell_text(r, 4, &format!("S{:02}", cabinet));
            table.set_cell_text(
                r,
                5,
                if step > 0 {
                    &step.to_string()
                } else {
                    "-"
                },
            );
        }

        host.begin_undo_group("MONTAZ: BOM tabuľka");
        let handle = host.add_entity(EntityType::Table(Box::new(table)));
        if !handle.is_null() {
            let meta = Meta {
                cabinet: 0,
                kind: ItemKind::Part,
                name: "BOM Montážny návod".to_string(),
                qty: 1,
                step: 0,
                role: "BOM".to_string(),
            };
            let _ = host.write_record(handle, meta_record(&meta));
            let _ = host.set_selection(&[handle]);
        }
        host.end_undo_group();

        if handle.is_null() {
            host.push_error("Montážny návod: BOM tabuľku sa nepodarilo vložiť.");
        } else {
            host.push_info("Montážny návod: BOM tabuľka bola vložená do DWG.");
        }
        self.rebuild_panel(host, true);
    }

    fn export_svg(&self, host: &mut dyn HostApi) {
        let step = self.state.lock().map(|s| s.step.max(1)).unwrap_or(1);
        host.push_info(&format!(
            "Montážny návod: export kroku {step}. Pre Affinity ulož na rovnakú cestu, napr. KROK_{step:02}.svg."
        ));
        run_host_command(host, "EXPORTSVG", "Export SVG pre Affinity");
    }
}

fn scan_drawing(host: &mut dyn HostApi) -> Vec<ScanEntry> {
    let mut out = Vec::new();
    let mut offset = 0u64;
    let mut ordinal = 1usize;

    loop {
        let query = format!(r#"{{"offset":{offset},"limit":1000}}"#);
        let page = match host.query_entities(&query) {
            Ok(page) => page,
            Err(_) => break,
        };
        if page.entities.is_empty() {
            break;
        }

        for entity in page.entities {
            if matches!(entity, EntityType::Table(_)) {
                let h = entity.as_entity().handle();
                if let Some(meta) = read_meta(host, h) {
                    if meta.role == "BOM" {
                        continue;
                    }
                }
            }
            let handle = entity.as_entity().handle();
            if !handle.is_valid() {
                continue;
            }
            out.push(scan_entity(host, &entity, ordinal));
            ordinal += 1;
        }

        match page.next_offset {
            Some(next) => offset = next,
            None => break,
        }
    }

    out.sort_by(|a, b| {
        a.cabinet
            .cmp(&b.cabinet)
            .then(a.kind.cmp(&b.kind))
            .then(a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    out
}

fn scan_one(host: &mut dyn HostApi, handle: Handle, ordinal: usize) -> ScanEntry {
    let entity = host.document().get_entity(handle).cloned();
    match entity {
        Some(entity) => scan_entity(host, &entity, ordinal),
        None => ScanEntry {
            handle,
            cabinet: 1,
            kind: ItemKind::Part,
            name: format!("Objekt {ordinal}"),
            qty: 1,
            step: 0,
            role: "MASTER".to_string(),
            layer: String::new(),
            tagged: false,
        },
    }
}

fn scan_entity(host: &dyn HostApi, entity: &EntityType, ordinal: usize) -> ScanEntry {
    let handle = entity.as_entity().handle();
    let layer = entity.as_entity().layer().to_string();
    let raw_name = match entity {
        EntityType::Insert(insert) if !insert.block_name.trim().is_empty() => insert.block_name.clone(),
        _ if !layer.trim().is_empty() && layer != "0" => layer.clone(),
        EntityType::Solid3D(_) => format!("3D diel {ordinal:03}"),
        EntityType::Body(_) => format!("Body {ordinal:03}"),
        EntityType::Mesh(_) => format!("Mesh {ordinal:03}"),
        _ => format!("Objekt {ordinal:03}"),
    };

    if let Some(meta) = read_meta(host, handle) {
        return ScanEntry {
            handle,
            cabinet: meta.cabinet.max(1),
            kind: meta.kind,
            name: if meta.name.trim().is_empty() {
                raw_name
            } else {
                meta.name
            },
            qty: meta.qty.max(1),
            step: meta.step.max(0),
            role: meta.role,
            layer,
            tagged: true,
        };
    }

    let kind = infer_kind(&raw_name, &layer);
    let cabinet = infer_cabinet(&raw_name, &layer);
    ScanEntry {
        handle,
        cabinet,
        kind,
        name: raw_name,
        qty: 1,
        step: 0,
        role: "MASTER".to_string(),
        layer,
        tagged: false,
    }
}

fn infer_kind(name: &str, layer: &str) -> ItemKind {
    let hay = format!("{} {}", name, layer).to_ascii_lowercase();
    let hardware_words = [
        "kovanie", "hardware", "fitting", "hinge", "zaves", "záves", "kolik", "kolík",
        "dowel", "screw", "vrut", "skrut", "bolt", "matica", "nut", "cam", "minifix",
        "rafix", "confirmat", "konfirm", "drawer slide", "výsuv", "vysuv", "handle",
        "uchyt", "úchyt", "pánt", "pant", "connector", "spoj",
    ];
    if hardware_words.iter().any(|word| hay.contains(word)) {
        ItemKind::Hardware
    } else {
        ItemKind::Part
    }
}

fn infer_cabinet(name: &str, layer: &str) -> i32 {
    let hay = format!("{} {}", name, layer).to_ascii_uppercase();
    for prefix in ["SKRINKA", "SKR", "CABINET", "CAB", "KORPUS"] {
        if let Some(pos) = hay.find(prefix) {
            let tail = &hay[pos + prefix.len()..];
            let digits: String = tail
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<i32>() {
                if n > 0 {
                    return n;
                }
            }
        }
    }
    1
}

fn meta_record(meta: &Meta) -> ExtendedDataRecord {
    ExtendedDataRecord {
        application_name: XDATA_APP.to_string(),
        values: vec![
            XDataValue::String("V=2".to_string()),
            XDataValue::String(format!("CAB={}", meta.cabinet)),
            XDataValue::String(format!("TYPE={}", meta.kind.as_str())),
            XDataValue::String(format!("NAME={}", meta.name)),
            XDataValue::String(format!("QTY={}", meta.qty)),
            XDataValue::String(format!("STEP={}", meta.step)),
            XDataValue::String(format!("ROLE={}", meta.role)),
        ],
    }
}

fn read_meta(host: &dyn HostApi, handle: Handle) -> Option<Meta> {
    let record = host.read_record(handle, XDATA_APP)?;

    let mut cabinet = 1;
    let mut kind = ItemKind::Part;
    let mut name = String::new();
    let mut qty = 1;
    let mut step = 0;
    let mut role = "MASTER".to_string();
    let mut keyed = false;

    for value in &record.values {
        match value {
            XDataValue::String(text) => {
                if let Some((key, value)) = text.split_once('=') {
                    keyed = true;
                    match key.to_ascii_uppercase().as_str() {
                        "CAB" => cabinet = value.parse().unwrap_or(1),
                        "TYPE" => {
                            kind = if value.eq_ignore_ascii_case("HARDWARE") {
                                ItemKind::Hardware
                            } else {
                                ItemKind::Part
                            }
                        }
                        "NAME" => name = value.to_string(),
                        "QTY" => qty = value.parse().unwrap_or(1),
                        "STEP" => step = value.parse().unwrap_or(0),
                        "ROLE" => role = value.to_ascii_uppercase(),
                        _ => {}
                    }
                }
            }
            XDataValue::Integer32(v) if !keyed && step == 0 => step = *v,
            XDataValue::Integer16(v) if !keyed && step == 0 => step = i32::from(*v),
            _ => {}
        }
    }

    Some(Meta {
        cabinet,
        kind,
        name,
        qty,
        step,
        role,
    })
}

fn build_panel(entries: &[ScanEntry], state_lock: &Mutex<PluginState>) -> PanelDef {
    let snapshot = state_lock
        .lock()
        .map(|s| {
            (
                s.cabinet,
                s.kind,
                s.name.clone(),
                s.qty,
                s.step,
                s.bom_x,
                s.bom_y,
                s.bom_z,
            )
        })
        .unwrap_or((1, ItemKind::Part, String::new(), 1, 1, 0.0, 0.0, 0.0));

    let tagged = entries.iter().filter(|e| e.tagged).count();
    let parts = entries.iter().filter(|e| e.kind == ItemKind::Part).count();
    let hardware = entries
        .iter()
        .filter(|e| e.kind == ItemKind::Hardware)
        .count();

    let mut panel = PanelDef::new(PANEL_ID, "Montážny návod")
        .with_width(380.0)
        .with_command("MONTAZ_PANEL")
        .with_control(DialogControl::InfoText {
            text: format!(
                "DWG: {} objektov | dielce: {} | kovanie: {} | označené metadátami: {}",
                entries.len(),
                parts,
                hardware,
                tagged
            ),
            align_h: None,
        })
        .with_control(DialogControl::Row {
            controls: vec![
                button("refresh", "Obnoviť strom", "MONTAZ_REFRESH"),
                button("load", "Načítať výber", "MONTAZ_LOAD_SELECTED"),
            ],
            align_h: None,
            align_v: None,
        })
        .with_control(DialogControl::Fields {
            id: "editor".to_string(),
            title: Some("Zaradenie objektu".to_string()),
            controls: vec![
                DialogControl::Number {
                    id: "cabinet_no".to_string(),
                    label: "Skrinka".to_string(),
                    default: snapshot.0 as f64,
                    min: Some(1.0),
                    max: Some(9999.0),
                    step: Some(1.0),
                },
                DialogControl::Choice {
                    id: "item_type".to_string(),
                    label: "Typ".to_string(),
                    options: vec!["Dielec".to_string(), "Kovanie".to_string()],
                    default_index: if snapshot.1 == ItemKind::Hardware { 1 } else { 0 },
                },
                DialogControl::Text {
                    id: "item_name".to_string(),
                    label: "Názov".to_string(),
                    default: snapshot.2,
                },
                DialogControl::Number {
                    id: "qty".to_string(),
                    label: "Množstvo".to_string(),
                    default: snapshot.3.max(1) as f64,
                    min: Some(1.0),
                    max: Some(99999.0),
                    step: Some(1.0),
                },
                DialogControl::Row {
                    controls: vec![
                        button("tag", "Zapísať označené", "MONTAZ_TAG_SELECTED"),
                        button("untag", "Odstrániť údaje", "MONTAZ_UNTAG_SELECTED"),
                    ],
                    align_h: None,
                    align_v: None,
                },
            ],
            collapsed: false,
        })
        .with_control(DialogControl::Fields {
            id: "montage".to_string(),
            title: Some("Montážne kroky".to_string()),
            controls: vec![
                DialogControl::Number {
                    id: "step_no".to_string(),
                    label: "Aktuálny krok".to_string(),
                    default: snapshot.4.max(1) as f64,
                    min: Some(1.0),
                    max: Some(9999.0),
                    step: Some(1.0),
                },
                DialogControl::Row {
                    controls: vec![
                        button("prev", "◀ Predch.", "MONTAZ_PREV_STEP"),
                        button("next", "Ďalší ▶", "MONTAZ_NEXT_STEP"),
                    ],
                    align_h: None,
                    align_v: None,
                },
                button("assign_step", "Priradiť výber k aktuálnemu kroku", "MONTAZ_ASSIGN_STEP"),
                DialogControl::Row {
                    controls: vec![
                        button("show_step", "Zobraziť krok", "MONTAZ_SHOW_STEP"),
                        button("show_new", "Iba nové", "MONTAZ_SHOW_ONLY_NEW"),
                    ],
                    align_h: None,
                    align_v: None,
                },
                button("show_all", "Zobraziť všetko", "MONTAZ_SHOW_ALL"),
            ],
            collapsed: false,
        })
        .with_control(DialogControl::Fields {
            id: "transform".to_string(),
            title: Some("Pracovné kópie a manipulácia".to_string()),
            controls: vec![
                button("work_copy", "Vytvoriť pracovnú kópiu", "MONTAZ_WORK_COPY"),
                DialogControl::Row {
                    controls: vec![
                        button("move", "MOVE", "MOVE"),
                        button("copy", "COPY", "COPY"),
                        button("rotate", "ROTATE", "ROTATE"),
                    ],
                    align_h: None,
                    align_v: None,
                },
                DialogControl::Row {
                    controls: vec![
                        button("move3d", "3DMOVE", "3DMOVE"),
                        button("rotate3d", "3DROTATE", "3DROTATE"),
                        button("align3d", "3DALIGN", "3DALIGN"),
                    ],
                    align_h: None,
                    align_v: None,
                },
            ],
            collapsed: false,
        })
        .with_control(DialogControl::Fields {
            id: "bom".to_string(),
            title: Some("BOM tabuľka do DWG".to_string()),
            controls: vec![
                DialogControl::Number {
                    id: "bom_x".to_string(),
                    label: "X".to_string(),
                    default: snapshot.5,
                    min: None,
                    max: None,
                    step: Some(10.0),
                },
                DialogControl::Number {
                    id: "bom_y".to_string(),
                    label: "Y".to_string(),
                    default: snapshot.6,
                    min: None,
                    max: None,
                    step: Some(10.0),
                },
                DialogControl::Number {
                    id: "bom_z".to_string(),
                    label: "Z".to_string(),
                    default: snapshot.7,
                    min: None,
                    max: None,
                    step: Some(10.0),
                },
                button("bom_insert", "Vložiť / vytvoriť BOM tabuľku", "MONTAZ_BOM_INSERT"),
            ],
            collapsed: true,
        })
        .with_control(DialogControl::Fields {
            id: "affinity".to_string(),
            title: Some("Affinity".to_string()),
            controls: vec![
                DialogControl::InfoText {
                    text: "SVG v Affinity vlož ako Linked resource. Export na rovnaký názov aktualizuje podklad.".to_string(),
                    align_h: None,
                },
                button("svg", "Export SVG aktuálneho pohľadu", "MONTAZ_EXPORT_SVG"),
            ],
            collapsed: false,
        });

    let mut cabinets: BTreeMap<i32, (Vec<(usize, &ScanEntry)>, Vec<(usize, &ScanEntry)>)> =
        BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let group = cabinets.entry(entry.cabinet).or_default();
        if entry.kind == ItemKind::Hardware {
            group.1.push((idx, entry));
        } else {
            group.0.push((idx, entry));
        }
    }

    for (cabinet, (parts, hardware)) in cabinets {
        let mut controls = Vec::new();

        if !parts.is_empty() {
            controls.push(DialogControl::Fields {
                id: format!("cab_{cabinet}_parts"),
                title: Some(format!("Dielce ({})", parts.len())),
                controls: parts
                    .into_iter()
                    .map(|(idx, entry)| tree_button(idx, entry))
                    .collect(),
                collapsed: false,
            });
        }

        if !hardware.is_empty() {
            controls.push(DialogControl::Fields {
                id: format!("cab_{cabinet}_hardware"),
                title: Some(format!("Kovanie ({})", hardware.len())),
                controls: hardware
                    .into_iter()
                    .map(|(idx, entry)| tree_button(idx, entry))
                    .collect(),
                collapsed: false,
            });
        }

        panel = panel.with_control(DialogControl::Fields {
            id: format!("cab_{cabinet}"),
            title: Some(format!("SKRINKA {:02}", cabinet)),
            controls,
            collapsed: cabinet != 1,
        });
    }

    let bom_rows = bom_rows(entries);
    panel.with_control(DialogControl::Table {
        id: "bom_preview".to_string(),
        label: Some("Kusovník – náhľad".to_string()),
        headers: vec![
            "Skr.".to_string(),
            "Typ".to_string(),
            "Názov".to_string(),
            "Qty".to_string(),
            "Krok".to_string(),
        ],
        rows: bom_rows,
        allow_add: false,
        allow_delete: false,
    })
}

fn button(id: &str, label: &str, action: &str) -> DialogControl {
    DialogControl::Button {
        id: id.to_string(),
        label: label.to_string(),
        action: Some(action.to_string()),
        align_h: None,
        width: None,
    }
}

fn tree_button(index: usize, entry: &ScanEntry) -> DialogControl {
    let tag = if entry.tagged { "●" } else { "○" };
    let role = if entry.role == "WORK" { " [KÓPIA]" } else { "" };
    let step = if entry.step > 0 {
        format!(" K{}", entry.step)
    } else {
        String::new()
    };
    button(
        &format!("tree_{index}"),
        &format!("{tag} {} ×{}{}{}", entry.name, entry.qty, step, role),
        &format!("MONTAZ_PICK_{index}"),
    )
}

fn bom_rows(entries: &[ScanEntry]) -> Vec<Vec<String>> {
    let mut bom: BTreeMap<(i32, ItemKind, String, i32), i32> = BTreeMap::new();
    for entry in entries {
        if entry.role == "WORK" || entry.role == "BOM" {
            continue;
        }
        *bom.entry((
            entry.cabinet,
            entry.kind,
            entry.name.clone(),
            entry.step.max(0),
        ))
        .or_insert(0) += entry.qty.max(1);
    }
    bom.into_iter()
        .map(|((cab, kind, name, step), qty)| {
            vec![
                format!("S{:02}", cab),
                kind.label().to_string(),
                name,
                qty.to_string(),
                if step > 0 {
                    step.to_string()
                } else {
                    "-".to_string()
                },
            ]
        })
        .collect()
}

fn update_editor_panel(host: &mut dyn HostApi, entry: &ScanEntry) {
    let mut values = HashMap::new();
    values.insert(
        "cabinet_no".to_string(),
        DialogValue::Text(entry.cabinet.to_string()),
    );
    values.insert(
        "item_type".to_string(),
        DialogValue::Choice {
            index: if entry.kind == ItemKind::Hardware { 1 } else { 0 },
            text: entry.kind.label().to_string(),
        },
    );
    values.insert(
        "item_name".to_string(),
        DialogValue::Text(entry.name.clone()),
    );
    values.insert("qty".to_string(), DialogValue::Text(entry.qty.to_string()));
    values.insert(
        "step_no".to_string(),
        DialogValue::Text(entry.step.max(1).to_string()),
    );
    host.update_panel(PANEL_ID, values);
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
                    "Montážny návod — {label}: {} — {}",
                    outcome.status,
                    outcome.prompt
                ));
            }
        }
        Err(error) => host.push_error(&format!("Montážny návod — {label}: {error}")),
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
                    title: "Projekt",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_PANEL",
                            label: "Strom\nzostavy",
                            icon: IconKind::Glyph("☰"),
                            event: ModuleEvent::Command("MONTAZ_PANEL".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_REFRESH",
                            label: "Obnoviť\nDWG",
                            icon: IconKind::Glyph("↻"),
                            event: ModuleEvent::Command("MONTAZ_REFRESH".to_string()),
                        }),
                    ],
                },
                RibbonGroup {
                    title: "Montáž",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_ASSIGN_STEP",
                            label: "Priradiť\nkrok",
                            icon: IconKind::Glyph("#"),
                            event: ModuleEvent::Command("MONTAZ_ASSIGN_STEP".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_SHOW_STEP",
                            label: "Zobraziť\nkrok",
                            icon: IconKind::Glyph("S"),
                            event: ModuleEvent::Command("MONTAZ_SHOW_STEP".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_WORK_COPY",
                            label: "Pracovná\nkópia",
                            icon: IconKind::Glyph("C"),
                            event: ModuleEvent::Command("MONTAZ_WORK_COPY".to_string()),
                        }),
                    ],
                },
                RibbonGroup {
                    title: "Výstup",
                    tools: vec![
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_BOM_INSERT",
                            label: "BOM\nDWG",
                            icon: IconKind::Glyph("T"),
                            event: ModuleEvent::Command("MONTAZ_BOM_INSERT".to_string()),
                        }),
                        RibbonItem::LargeTool(ToolDef {
                            id: "MONTAZ_EXPORT_SVG",
                            label: "Affinity\nSVG",
                            icon: IconKind::Glyph("SVG"),
                            event: ModuleEvent::Command("MONTAZ_EXPORT_SVG".to_string()),
                        }),
                    ],
                },
            ]
        })
    }
}

export_plugin!(MontaznyNavodPlugin);
