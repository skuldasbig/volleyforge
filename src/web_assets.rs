//! Web asset loading for VolleyForge.
//!
//! Because the web build must not ship proprietary original files inside the
//! `.wasm` binary or on the web server, the web version allows the user to
//! provide their own `blobby.zip` or the three `.pak` containers via drag-and-drop
//! or a file upload dialog.
//!
//! Extracted containers are held in memory and optionally persisted to
//! the browser's `localStorage` (base64 encoded) so returning players
//! don't need to re-upload on every reload.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;
use eframe::egui;
use crate::assets::RawContainers;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Event, FileReader, HtmlInputElement};

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut res = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        res.push(B64_CHARS[(b0 >> 2) as usize] as char);
        res.push(B64_CHARS[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            res.push(B64_CHARS[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            res.push('=');
        }
        if chunk.len() > 2 {
            res.push(B64_CHARS[(b2 & 0x3f) as usize] as char);
        } else {
            res.push('=');
        }
    }
    res
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in s.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

pub fn load_cached_assets() -> Option<RawContainers> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let g_b64 = storage.get_item("vf_graph_pak").ok()??;
    let s_b64 = storage.get_item("vf_sound_pak").ok()??;
    let t_b64 = storage.get_item("vf_text_pak").ok()??;
    let graph = base64_decode(&g_b64)?;
    let sound = base64_decode(&s_b64)?;
    let text = base64_decode(&t_b64)?;
    if graph.is_empty() || sound.is_empty() || text.is_empty() {
        return None;
    }
    Some(RawContainers { graph, sound, text })
}

pub fn save_cached_assets(raw: &RawContainers) {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.set_item("vf_graph_pak", &base64_encode(&raw.graph));
            let _ = storage.set_item("vf_sound_pak", &base64_encode(&raw.sound));
            let _ = storage.set_item("vf_text_pak", &base64_encode(&raw.text));
        }
    }
}

pub fn clear_cached_assets() {
    if let Some(window) = web_sys::window() {
        if let Ok(Some(storage)) = window.local_storage() {
            let _ = storage.remove_item("vf_graph_pak");
            let _ = storage.remove_item("vf_sound_pak");
            let _ = storage.remove_item("vf_text_pak");
        }
    }
}

fn trigger_file_dialog(target: Rc<RefCell<Vec<(String, Vec<u8>)>>>) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    let Ok(elem) = document.create_element("input") else { return };
    let Ok(input) = elem.dyn_into::<HtmlInputElement>() else { return };

    input.set_type("file");
    input.set_accept(".zip,.pak");
    input.set_multiple(true);
    let _ = input.style().set_property("display", "none");

    if let Some(body) = document.body() {
        let _ = body.append_child(&input);
    }

    let input_clone = input.clone();
    let onchange = Closure::wrap(Box::new(move |_e: Event| {
        let Some(files) = input_clone.files() else { return };
        for i in 0..files.length() {
            let Some(file) = files.get(i) else { continue };
            let name = file.name();
            let Ok(reader) = FileReader::new() else { continue };
            let reader_clone = reader.clone();
            let target_inner = target.clone();
            let onload = Closure::wrap(Box::new(move |_e: Event| {
                if let Ok(res) = reader_clone.result() {
                    let array = js_sys::Uint8Array::new(&res);
                    target_inner.borrow_mut().push((name.clone(), array.to_vec()));
                }
            }) as Box<dyn FnMut(Event)>);
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            let _ = reader.read_as_array_buffer(&file);
        }
    }) as Box<dyn FnMut(Event)>);

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

pub struct WebAssetManager {
    pub graph: Option<Vec<u8>>,
    pub sound: Option<Vec<u8>>,
    pub text: Option<Vec<u8>>,
    pub persist: bool,
    pub status: Option<String>,
    pub error: Option<String>,
    pending: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
}

impl Default for WebAssetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WebAssetManager {
    pub fn new() -> Self {
        Self {
            graph: None,
            sound: None,
            text: None,
            persist: true,
            status: None,
            error: None,
            pending: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn ready_containers(&self) -> Option<RawContainers> {
        match (&self.graph, &self.sound, &self.text) {
            (Some(g), Some(s), Some(t)) => Some(RawContainers {
                graph: g.clone(),
                sound: s.clone(),
                text: t.clone(),
            }),
            _ => None,
        }
    }

    pub fn open_file_dialog(&self) {
        trigger_file_dialog(self.pending.clone());
    }

    pub fn reset(&mut self) {
        self.graph = None;
        self.sound = None;
        self.text = None;
        self.status = None;
        self.error = None;
        self.pending.borrow_mut().clear();
        clear_cached_assets();
    }

    pub fn update(&mut self, ctx: &egui::Context) -> Option<RawContainers> {
        // 1. Process files from file picker
        let files_from_picker: Vec<(String, Vec<u8>)> = self.pending.borrow_mut().drain(..).collect();
        for (name, bytes) in files_from_picker {
            inspect_and_add_file(&name, &bytes, self);
        }

        // 2. Process drag & drop from egui
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let name = file.path().to_string_lossy().to_string();
            let target_inner = self.pending.clone();
            let file_clone = file.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(bytes) = file_clone.bytes_async().await {
                    target_inner.borrow_mut().push((name, bytes));
                }
            });
        }

        // 3. If all complete, optionally save and return
        if let Some(raw) = self.ready_containers() {
            if self.persist {
                save_cached_assets(&raw);
            }
            return Some(raw);
        }

        None
    }
}

fn inspect_and_add_file(name: &str, bytes: &[u8], state: &mut WebAssetManager) {
    let lower = name.to_lowercase();
    let is_zip = lower.ends_with(".zip") || (bytes.len() >= 4 && &bytes[0..4] == b"PK\x03\x04");

    if is_zip {
        match zip::ZipArchive::new(Cursor::new(bytes)) {
            Ok(mut archive) => {
                let mut found_count = 0;
                for i in 0..archive.len() {
                    if let Ok(mut entry) = archive.by_index(i) {
                        let Ok(raw_name) = entry.name() else { continue };
                        let entry_name = raw_name.to_lowercase();
                        let mut data = Vec::with_capacity(entry.size() as usize);
                        if std::io::copy(&mut entry, &mut data).is_ok() {
                            if entry_name.ends_with("graph.pak") {
                                state.graph = Some(data);
                                found_count += 1;
                            } else if entry_name.ends_with("sound.pak") {
                                state.sound = Some(data);
                                found_count += 1;
                            } else if entry_name.ends_with("text.pak") {
                                state.text = Some(data);
                                found_count += 1;
                            }
                        }
                    }
                }
                if found_count > 0 {
                    state.status = Some(format!("ZIP erfolgreich entpackt: {found_count} PAK-Dateien gefunden."));
                    state.error = None;
                } else {
                    state.error = Some("In der ZIP-Datei wurden keine .pak-Dateien gefunden (erwartet: graph.pak, sound.pak, text.pak).".to_string());
                }
            }
            Err(e) => {
                state.error = Some(format!("Fehler beim Entpacken der ZIP-Datei: {e}"));
            }
        }
    } else if lower.ends_with("graph.pak") {
        state.graph = Some(bytes.to_vec());
        state.status = Some("graph.pak übernommen.".to_string());
        state.error = None;
    } else if lower.ends_with("sound.pak") {
        state.sound = Some(bytes.to_vec());
        state.status = Some("sound.pak übernommen.".to_string());
        state.error = None;
    } else if lower.ends_with("text.pak") {
        state.text = Some(bytes.to_vec());
        state.status = Some("text.pak übernommen.".to_string());
        state.error = None;
    } else {
        state.error = Some(format!("Unerwartete Datei '{name}'. Bitte blobby.zip oder die drei .pak-Dateien verwenden."));
    }
}

/// Renders the upload / drop screen when assets are missing on the web platform.
pub fn draw_web_asset_upload(ui: &mut egui::Ui, manager: &mut WebAssetManager) {
    let size = ui.available_size();

    ui.vertical_centered(|ui| {
        ui.add_space((size.y * 0.08).max(10.0));

        ui.label(
            egui::RichText::new("🏐 VOLLEYFORGE")
                .size(32.0)
                .strong()
                .color(egui::Color32::from_rgb(255, 215, 0)),
        );
        ui.label(
            egui::RichText::new("Original-Assets benötigt")
                .size(20.0)
                .strong()
                .color(egui::Color32::WHITE),
        );

        ui.add_space(12.0);

        egui::Frame::window(ui.style())
            .fill(egui::Color32::from_rgba_premultiplied(20, 24, 32, 240))
            .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(60, 90, 130)))
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_max_width(520.0);

                ui.label(
                    egui::RichText::new(
                        "Aus Respekt vor dem Urheberrecht enthält diese Web-Version keine originalen Spieldateien.\n\n\
                        Ziehe einfach die originale blobby.zip oder die drei .pak-Dateien (graph.pak, sound.pak, text.pak) \
                        direkt in dieses Fenster:"
                    )
                    .size(14.0)
                    .color(egui::Color32::from_rgb(210, 220, 230)),
                );

                ui.add_space(14.0);

                // Drag & Drop Dropzone Box
                let drop_zone_color = egui::Color32::from_rgb(35, 45, 60);
                egui::Frame::group(ui.style())
                    .fill(drop_zone_color)
                    .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 130, 200)))
                    .corner_radius(6.0)
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new("📥 Datei hier ablegen (Drag & Drop)")
                                    .size(16.0)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new("Unterstützt: blobby.zip oder graph.pak, sound.pak, text.pak")
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(160, 180, 200)),
                            );
                            ui.add_space(8.0);
                            if ui.button("📂 Datei(en) auf dem Computer auswählen...").clicked() {
                                manager.open_file_dialog();
                            }
                        });
                    });

                ui.add_space(14.0);

                // Container status table
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Status der Container:").strong().color(egui::Color32::WHITE));
                });
                ui.add_space(4.0);

                let render_status = |ui: &mut egui::Ui, name: &str, opt: &Option<Vec<u8>>| {
                    ui.horizontal(|ui| {
                        if let Some(bytes) = opt {
                            ui.label(
                                egui::RichText::new(format!("  ✅ {name} ({} KB)", bytes.len() / 1024))
                                    .color(egui::Color32::from_rgb(100, 255, 100)),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(format!("  ⬜ {name} (fehlt)"))
                                    .color(egui::Color32::from_rgb(200, 200, 200)),
                            );
                        }
                    });
                };

                render_status(ui, "graph.pak (Grafiken & Fonts)", &manager.graph);
                render_status(ui, "sound.pak (Soundeffekte)", &manager.sound);
                render_status(ui, "text.pak (Texte & Menüs)", &manager.text);

                if manager.graph.is_some() || manager.sound.is_some() || manager.text.is_some() {
                    ui.add_space(4.0);
                    if ui.button("↺ Auswahl zurücksetzen / Cache leeren").clicked() {
                        manager.reset();
                    }
                }

                ui.add_space(12.0);

                ui.checkbox(
                    &mut manager.persist,
                    egui::RichText::new("Im Browser speichern (LocalStorage, kein erneuter Upload nötig)").size(13.0),
                );

                if let Some(err) = &manager.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(err).color(egui::Color32::from_rgb(255, 100, 100)).size(13.0));
                } else if let Some(status) = &manager.status {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(status).color(egui::Color32::from_rgb(100, 255, 100)).size(13.0));
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Noch keine Assets?").size(12.0).color(egui::Color32::GRAY));
                    ui.hyperlink_to(
                        "🔗 blobby.zip auf Archive.org herunterladen",
                        "https://archive.org/download/blobbyvolley_202102/blobby.zip",
                    );
                });
            });
    });
}
