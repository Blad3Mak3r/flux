use super::*;

/// A fader of a layer (a group has the first two only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Master,
    Opacity,
    Volume,
    Transition,
}

/// What the user can do to the grid. Every id names something of the grid; a name that is empty
/// after trimming goes back to the default one. A level out of its range is brought into it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VjOp {
    /// A new empty layer on top of the others, global or of the decks.
    AddLayer {
        #[serde(default)]
        global: bool,
    },
    /// Takes the layer and its clips away; the last layer stays.
    RemoveLayer {
        layer: String,
    },
    /// Puts the layer at `to` of the drawing order, 0 being the bottom.
    MoveLayer {
        layer: String,
        to: usize,
    },
    RenameLayer {
        layer: String,
        name: String,
    },
    LayerMaster {
        layer: String,
        value: f32,
    },
    LayerOpacity {
        layer: String,
        value: f32,
    },
    LayerVolume {
        layer: String,
        value: f32,
    },
    /// Brings one fader of the layer back to the value a new layer has.
    LayerLevelReset {
        layer: String,
        level: Level,
    },
    LayerBlend {
        layer: String,
        blend: Blend,
    },
    /// In seconds.
    LayerTransition {
        layer: String,
        value: f32,
    },
    /// With `random`, every transition of the layer picks its own blend and `effect` is kept for
    /// when it is turned off.
    LayerTransitionEffect {
        layer: String,
        effect: Blend,
        #[serde(default)]
        random: bool,
    },
    LayerCrossfade {
        layer: String,
        side: Option<Side>,
    },
    /// What the layer launches by itself at the end of a clip's pass.
    LayerAutopilot {
        layer: String,
        mode: Autopilot,
    },
    LayerBypass {
        layer: String,
        on: bool,
    },
    LayerSolo {
        layer: String,
        on: bool,
    },
    /// Folds the layer's row on the grid, or unfolds it.
    LayerCollapse {
        layer: String,
        on: bool,
    },
    /// Puts the layers in a new group (Resolume's New Group): they gather where the lowest of
    /// them is, in their order, and leave the groups they were in.
    GroupLayers {
        layers: Vec<String>,
    },
    /// Puts the layer in `group`, on its top unless `to` says where in the drawing order, or in
    /// none. A group never stays empty: the layer that was its last leaves a new empty one behind.
    LayerGroup {
        layer: String,
        group: Option<String>,
        to: Option<usize>,
    },
    /// Takes the group away; its layers stay as they are, outside any group.
    Ungroup {
        group: String,
    },
    RenameGroup {
        group: String,
        name: String,
    },
    GroupMaster {
        group: String,
        value: f32,
    },
    GroupOpacity {
        group: String,
        value: f32,
    },
    /// Brings the group's master or video fader back to the value a new group has.
    GroupLevelReset {
        group: String,
        level: Level,
    },
    GroupBlend {
        group: String,
        blend: Blend,
    },
    GroupBypass {
        group: String,
        on: bool,
    },
    GroupSolo {
        group: String,
        on: bool,
    },
    /// Folds the group's header and layers on the grid, or unfolds them.
    GroupCollapse {
        group: String,
        on: bool,
    },
    /// The tempo of the show, in beats a minute.
    SetBpm {
        bpm: f64,
    },
    /// The tempo moved by `by` beats a minute.
    NudgeBpm {
        by: f64,
    },
    /// The tempo times `factor`, as the ×2 and ÷2 buttons.
    ScaleBpm {
        factor: f64,
    },
    /// The tempo of the taps on the tap button, in milliseconds (see [`tapped_bpm`]).
    TapBpm {
        taps: Vec<f64>,
    },
    /// Where the timecode clock comes from.
    SetClock {
        source: ClockSource,
    },
    /// Joins the Ableton Link session, or leaves it.
    SetLink {
        on: bool,
    },
    /// A new deck with [`DEFAULT_COLUMNS`] empty columns, after the others, which the grid shows.
    AddDeck,
    /// Takes the deck, its columns and their clips away; the last deck stays.
    RemoveDeck {
        deck: String,
    },
    MoveDeck {
        deck: String,
        to: usize,
    },
    RenameDeck {
        deck: String,
        name: String,
    },
    /// Shows the deck; what plays keeps playing.
    SelectDeck {
        deck: String,
    },
    /// A new empty column on the right of the deck the grid shows.
    AddColumn,
    /// Takes the column and its clips away; the last column of a deck stays.
    RemoveColumn {
        column: String,
    },
    /// Moves the column to `to` within its deck.
    MoveColumn {
        column: String,
        to: usize,
    },
    RenameColumn {
        column: String,
        name: String,
    },
    /// Puts a video of the project in a cell, in place of the clip that was there. The cell is the
    /// layer's in `column`; on a global layer, the slot at the place of `column` in its deck.
    PlaceVideo {
        asset: String,
        layer: String,
        column: String,
    },
    /// Puts a new generator in a cell, named as for [`VjOp::PlaceVideo`].
    PlaceGenerator {
        generator: Generator,
        layer: String,
        column: String,
    },
    /// Moves a clip to a cell, named as for [`VjOp::PlaceVideo`]; a clip already there takes the
    /// place the moved one left.
    MoveClip {
        clip: String,
        layer: String,
        column: String,
    },
    RemoveClip {
        clip: String,
    },
    RenameClip {
        clip: String,
        name: String,
    },
    ClipPlayback {
        clip: String,
        playback: Playback,
    },
    /// How fast the clip plays, times its own speed, from [`MIN_SPEED`] to [`MAX_SPEED`].
    ClipSpeed {
        clip: String,
        value: f32,
    },
    /// How long a still image or a generator lasts, in seconds, from [`MIN_DURATION`] to
    /// [`MAX_DURATION`]; a video keeps the length of its file.
    ClipDuration {
        clip: String,
        value: f32,
    },
    ClipDirection {
        clip: String,
        direction: Direction,
    },
    /// Free, or a pass of so many beats; the beats go to the nearest of [`TEMPO_BEATS`].
    ClipTempo {
        clip: String,
        tempo: Tempo,
    },
    /// The part of the clip that plays, as fractions of its length; cues stay where they are.
    ClipRegion {
        clip: String,
        start: f32,
        end: f32,
    },
    /// Puts cue `index` (0 to 7) of the clip at `position`, a fraction of its length, or empties
    /// it with `None`.
    ClipCue {
        clip: String,
        index: usize,
        position: Option<f32>,
    },
    /// Changes what a generator clip draws; a video clip stays a video.
    ClipGenerator {
        clip: String,
        generator: Generator,
    },
    /// Moves the line a text block shows (its `line`) by `step` lines, skipping the empty ones and
    /// wrapping round. A video clip or another generator refuses it.
    TextLine {
        clip: String,
        step: i32,
    },
    ClipTrigger {
        clip: String,
        trigger: Trigger,
    },
    ClipFit {
        clip: String,
        fit: VideoFit,
    },
    /// Draws the clip `value` times its fitted size (1 is 100%), up to [`MAX_SCALE`].
    ClipScale {
        clip: String,
        value: f32,
    },
    /// Moves the clip's centre from where its fit puts it, in fractions of the composition, up to
    /// [`MAX_OFFSET`] either way.
    ClipPosition {
        clip: String,
        x: f32,
        y: f32,
    },
    /// Puts a new effect of the kind called `kind` (see [`crate::vj_effects::catalog`]), with the settings
    /// it starts with, at the end of the stack of a clip, of a layer or of the composition; a stack
    /// holds at most [`MAX_EFFECTS`]. The kinds that are for the composition only are refused on a
    /// clip or a layer.
    AddEffect {
        target: EffectTarget,
        kind: String,
    },
    /// Skips (or brings back) the whole composition stack, whatever each effect's own bypass says.
    CompositionBypass {
        bypass: bool,
    },
    RemoveEffect {
        effect: String,
    },
    /// Puts the effect at `index` of its stack, 0 being the first applied.
    MoveEffect {
        effect: String,
        index: usize,
    },
    /// Gives the effect the settings of a preset of its kind (see [`crate::vj_presets`]); what the
    /// preset lacks keeps its value.
    ApplyEffectPreset {
        effect: String,
        preset: crate::vj_presets::EffectPreset,
    },
    /// Puts the effects of a preset, with new ids, at the end of a stack.
    AddEffectPreset {
        target: EffectTarget,
        preset: crate::vj_presets::EffectPreset,
    },
    /// Sets the effect's settings; `kind` is the kind it already is.
    EffectParams {
        effect: String,
        kind: EffectKind,
    },
    EffectBypass {
        effect: String,
        bypass: bool,
    },
    /// Puts a copy of the effect, with a new id and the same settings, right after it; a stack
    /// holds at most [`MAX_EFFECTS`].
    DuplicateEffect {
        effect: String,
    },
    /// Gives the effect the settings its kind starts with, and takes its modulation away.
    ResetEffect {
        effect: String,
    },
    /// Sets what drives a numeric setting by itself (see [`crate::vj_modulation`]), or with `None`
    /// takes it away. The values are brought into the setting's range.
    SetModulation {
        target: ModTarget,
        modulation: Option<Modulation>,
    },
    /// Makes `source` (a MIDI control or an OSC address, as learned) drive `target`. Latest wins: a
    /// mapping of the same source or of the same target is replaced, and a target that is mapped
    /// again keeps the range, inversion and mode it had.
    AddMapping {
        source: Source,
        target: Target,
    },
    RemoveMapping {
        mapping: String,
    },
    /// Takes away the mapping of `target`, if it has one.
    ClearMapping {
        target: Target,
    },
    /// Sets the options of a mapping: the range it scales into (in the target's own units),
    /// whether it is inverted, and what a button does.
    SetMapping {
        mapping: String,
        min: f32,
        max: f32,
        invert: bool,
        mode: ButtonMode,
    },
    /// Pairs a DMX mapping with the `fine` channel of its 16-bit value, or back to 8 bits with
    /// `None`. Only a DMX source has one.
    SetMappingFine {
        mapping: String,
        fine: Option<u16>,
    },
    /// Sets the values a mapping's pad is sent for its states, or with `None` takes them away so
    /// the grid's colours apply.
    SetMappingLights {
        mapping: String,
        lights: Option<crate::vj_feedback::Lights>,
    },
    /// Sets the colours the pads of mappings with none of their own get, or with `None` goes
    /// back to the controller's preset.
    SetLights {
        lights: Option<crate::vj_feedback::Lights>,
    },
    /// Sets where the OSC feedback goes: `host:port` separated by commas or spaces, at most 4;
    /// an empty text turns it off.
    SetOscFeedback {
        destinations: String,
    },
    /// Sets how a mapping of a clip, layer, column, deck or effect follows it: by id, or by the
    /// slot it is in.
    SetMappingFollow {
        mapping: String,
        follow: crate::vj_mapping::Follow,
    },
    /// Turns a knob of the dashboard to `value` (0 to 1) and moves every number linked to it.
    DashboardValue {
        /// Whose dashboard; absent is the composition's.
        #[serde(default)]
        scope: Scope,
        knob: usize,
        value: f32,
    },
    /// Links a number to a knob (or changes the range of an existing link): the knob at 0 gives
    /// `min` and at 1 gives `max`. A bound left out is the target's own, so a new link covers its
    /// whole range.
    DashboardLink {
        #[serde(default)]
        scope: Scope,
        knob: usize,
        target: Target,
        min: Option<f32>,
        max: Option<f32>,
    },
    DashboardUnlink {
        #[serde(default)]
        scope: Scope,
        knob: usize,
        target: Target,
    },
    RenameKnob {
        #[serde(default)]
        scope: Scope,
        knob: usize,
        name: String,
    },
}

impl VjOp {
    /// Whether the op only moves a value, so that a fader or a controller can send it many times
    /// a second without the project being copied and checked each time (see `Session::preview_vj`).
    pub fn is_preview(&self) -> bool {
        matches!(
            self,
            Self::LayerMaster { .. }
                | Self::LayerOpacity { .. }
                | Self::GroupMaster { .. }
                | Self::GroupOpacity { .. }
                | Self::LayerVolume { .. }
                | Self::LayerTransition { .. }
                | Self::ClipScale { .. }
                | Self::ClipPosition { .. }
                | Self::ClipSpeed { .. }
                | Self::ClipRegion { .. }
                | Self::ClipGenerator { .. }
                | Self::EffectParams { .. }
                | Self::SetModulation { .. }
                | Self::DashboardValue { .. }
        )
    }

    /// Whether the op can take something a mapping or a link names out of the grid.
    pub(super) fn may_remove(&self) -> bool {
        matches!(
            self,
            Self::RemoveLayer { .. }
                | Self::GroupLayers { .. }
                | Self::LayerGroup { .. }
                | Self::Ungroup { .. }
                | Self::RemoveDeck { .. }
                | Self::RemoveColumn { .. }
                | Self::RemoveClip { .. }
                | Self::RemoveEffect { .. }
                | Self::PlaceVideo { .. }
                | Self::PlaceGenerator { .. }
                | Self::MoveClip { .. }
                | Self::ClipGenerator { .. }
        )
    }
}
