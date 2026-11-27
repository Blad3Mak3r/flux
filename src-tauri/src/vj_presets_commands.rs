//! The user's effect presets and show templates: where they are kept (the app data folder) and the
//! commands that list, save, apply, rename and delete them. The rules are in
//! `flux_project::vj_presets`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::commands::{SharedSession, run};
use crate::recovery::{self, SharedRecoveries};
use crate::transport_commands::{SharedOutput, release};
use flux_audio::SharedAudio;
use flux_project::vj::{EffectTarget, VjOp};
use flux_project::vj_presets::{self, EffectPreset, Library, Template, TemplateInfo};
use flux_project::{Project, ProjectError, ProjectState, Result};

/// Commands read and rewrite the whole library file, and windows can ask at the same time.
static LIBRARY: Mutex<()> = Mutex::new(());

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|err| ProjectError::Invalid(format!("there is no app data folder: {err}")))
}

fn library_file(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("vj-presets.json"))
}

fn templates_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("vj-templates"))
}

/// Reads the library, lets `edit` change it and writes it back; answers with the presets.
fn edit_library(
    app: &AppHandle,
    edit: impl FnOnce(&mut Library) -> Result<()>,
) -> Result<Vec<EffectPreset>> {
    let _guard = LIBRARY.lock().unwrap_or_else(PoisonError::into_inner);
    let file = library_file(app)?;
    let mut library = Library::load(&file)?;
    edit(&mut library)?;
    library.save(&file)?;
    Ok(library.presets)
}

/// What a preset is made from: one effect, or the whole stack of a clip, a layer, a group or the
/// composition.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresetSource {
    Effect(String),
    Stack(EffectTarget),
}

/// What a preset is used on: an effect of its kind, or the end of a stack.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresetTo {
    Effect(String),
    Stack(EffectTarget),
}

#[tauri::command]
pub async fn vj_presets_list(app: AppHandle) -> Result<Vec<EffectPreset>> {
    blocking(move || {
        let _guard = LIBRARY.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(Library::load(&library_file(&app)?)?.presets)
    })
    .await
}

#[tauri::command]
pub async fn vj_preset_save(
    app: AppHandle,
    project: Project,
    source: PresetSource,
    name: String,
) -> Result<Vec<EffectPreset>> {
    blocking(move || {
        let id = Uuid::new_v4().to_string();
        let presets = edit_library(&app, |library| {
            let preset = match &source {
                PresetSource::Effect(effect) => {
                    EffectPreset::of_effect(&project.vj, effect, id, &name)?
                }
                PresetSource::Stack(target) => {
                    EffectPreset::of_stack(&project.vj, target, id, &name)?
                }
            };
            library.add(preset)
        })?;
        log::info!("effect preset {name:?} saved");
        Ok(presets)
    })
    .await
}

#[tauri::command]
pub async fn vj_preset_rename(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<EffectPreset>> {
    blocking(move || edit_library(&app, |library| library.rename(&id, &name))).await
}

#[tauri::command]
pub async fn vj_preset_delete(app: AppHandle, id: String) -> Result<Vec<EffectPreset>> {
    blocking(move || edit_library(&app, |library| library.remove(&id))).await
}

/// Uses a preset: it sets the settings of an effect of its kind, or its effects are added to a
/// stack. The answer is the project with it done, one undo step.
#[tauri::command]
pub async fn vj_preset_apply(
    app: AppHandle,
    session: State<'_, SharedSession>,
    project: Project,
    id: String,
    to: PresetTo,
) -> Result<ProjectState> {
    let preset = blocking(move || {
        let _guard = LIBRARY.lock().unwrap_or_else(PoisonError::into_inner);
        Library::load(&library_file(&app)?)?.get(&id).cloned()
    })
    .await?;
    let state = run(&session, move |s| {
        s.update(project);
        s.edit_vj(match to {
            PresetTo::Effect(effect) => VjOp::ApplyEffectPreset { effect, preset },
            PresetTo::Stack(target) => VjOp::AddEffectPreset { target, preset },
        })
    })
    .await?;
    crate::modulation::wake();
    Ok(state)
}

#[tauri::command]
pub async fn vj_templates_list(app: AppHandle) -> Result<Vec<TemplateInfo>> {
    blocking(move || Ok(vj_presets::list_templates(&templates_dir(&app)?))).await
}

/// Saves the structure of the VJ show being edited, without its clips, as a template.
#[tauri::command]
pub async fn vj_template_save(
    app: AppHandle,
    project: Project,
    name: String,
) -> Result<Vec<TemplateInfo>> {
    blocking(move || {
        let dir = templates_dir(&app)?;
        let template = Template::of(&project, &name)?;
        vj_presets::save_template(&dir, &template, Uuid::new_v4().to_string())?;
        log::info!("template {name:?} saved");
        Ok(vj_presets::list_templates(&dir))
    })
    .await
}

#[tauri::command]
pub async fn vj_template_rename(
    app: AppHandle,
    id: String,
    name: String,
) -> Result<Vec<TemplateInfo>> {
    blocking(move || {
        let dir = templates_dir(&app)?;
        vj_presets::rename_template(&dir, &id, &name)?;
        Ok(vj_presets::list_templates(&dir))
    })
    .await
}

#[tauri::command]
pub async fn vj_template_delete(app: AppHandle, id: String) -> Result<Vec<TemplateInfo>> {
    blocking(move || {
        let dir = templates_dir(&app)?;
        vj_presets::delete_template(&dir, &id)?;
        Ok(vj_presets::list_templates(&dir))
    })
    .await
}

/// Starts a new VJ show with the structure of a template, as `project_new` starts a blank one.
#[expect(
    clippy::too_many_arguments,
    reason = "injected state and arguments of a command"
)]
#[tauri::command]
pub async fn vj_template_new(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
    id: String,
    name: String,
) -> Result<ProjectState> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .ensure_editable()?;
    let _voice_transition = voice.begin_transition()?;
    let dir = templates_dir(&app)?;
    let template = blocking(move || vj_presets::load_template(&dir, &id)).await?;
    release(&output).await;
    audio.clear();
    log::info!("new show {name:?} from template {:?}", template.name);
    recovery::transition(&session, &store, move |s| {
        let project = s.new_project_from_template(&template, &name)?;
        Ok(ProjectState {
            project,
            active: s.active(),
        })
    })
    .await
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| ProjectError::Io(std::io::Error::other(err)))?
}
