# Media API Layer — Architecture Sketch

> Workwright as a middle layer between human intent and generative media APIs.

## The Idea

Right now, Workwright mediates between a human and a codebase. The wright reads files, makes changes, defends its choices. The same loop works for *any* generative output — video, images, audio — if you replace "write code" with "call an API."

The human never writes a Runway prompt. They brief a shot. The wright translates intent into API parameters, defends those choices, generates the output, and submits it for crit. Taste accumulates around what works visually, not just what works technically.

## Why This Matters

Most people using Runway/Midjourney/DALL-E right now:
- Write prompts by trial and error
- Have no memory of what worked or why
- Can't articulate their aesthetic (moodboard ≠ taste)
- Repeat the same mistakes across sessions

A CD on a shoot doesn't say "make the fog 0.7 opacity with a 5500K color temperature." They say "I want this to feel like Stalker — the shot where he's lying in the water." The translation from intent to technical parameters is craft knowledge. That's what the wright learns.

## Loop (Same Three Words)

```
Brief → Defense → Crit
  │         │         │
  ▼         ▼         ▼
Intent   API params   Taste signal
         + rationale  on the OUTPUT
```

1. **Brief:** "Slow dolly push into a fog-covered field at dawn. Tarkovsky pacing. The camera should feel like it's remembering something."
2. **Wright translates:** prompt text, motion parameters, duration, aspect ratio, style references, model selection — and defends each choice.
3. **Generate:** Wright calls the API. Output (video/image/audio) goes to staging.
4. **Crit:** Human watches the output, reads the defense. Scores it. "The fog density is right but the light is too warm for dawn. The motion is too smooth — needs more handheld drift."
5. **Taste accumulates:** After 50 crits, the wright knows desaturated > saturated, natural motion > AI-smooth, 16:9 default, longer durations, no stock-footage-energy.

## Architecture

### New Crate: `ww-media`

Abstraction layer over generative media APIs.

```rust
/// A media provider that can generate from a brief.
pub trait MediaProvider: Send + Sync {
    /// Provider name (e.g., "runway", "dalle", "luma")
    fn name(&self) -> &str;

    /// What kinds of output this provider produces
    fn output_kind(&self) -> OutputKind;

    /// Translate a brief + taste context into provider-specific parameters
    /// Returns the parameters AND a defense of the translation choices
    async fn translate(
        &self,
        brief: &MediaBrief,
        taste: &str,
        llm: &LlmClient,
    ) -> Result<Translation>;

    /// Execute the generation with the translated parameters
    async fn generate(&self, params: &Translation) -> Result<MediaOutput>;

    /// Estimated cost for this generation (for budget awareness)
    fn estimate_cost(&self, params: &Translation) -> Option<f64>;
}

pub enum OutputKind {
    Video,
    Image,
    Audio,
}

pub struct MediaBrief {
    pub intent: String,       // "slow dolly push into fog-covered field"
    pub why: String,          // "establishing shot — the landscape is a character"
    pub references: Vec<String>,  // "Tarkovsky", "Stalker water scene"
    pub constraints: MediaConstraints,
}

pub struct MediaConstraints {
    pub duration: Option<f32>,     // seconds
    pub aspect_ratio: Option<String>,  // "16:9", "9:16", "1:1"
    pub resolution: Option<String>,    // "1080p", "4k"
    pub style: Option<String>,         // freeform style direction
}

pub struct Translation {
    pub provider: String,
    pub params: serde_json::Value,  // provider-specific (prompt, motion, etc.)
    pub defense: String,            // why these specific parameters
    pub estimated_cost: Option<f64>,
}

pub struct MediaOutput {
    pub path: PathBuf,          // local file path (staging)
    pub format: String,         // "mp4", "png", "wav"
    pub metadata: serde_json::Value,  // provider response metadata
    pub generation_time: f64,   // seconds
}
```

### Provider Implementations

```
ww-media/
  src/
    lib.rs          # trait + types
    runway.rs       # Runway Gen-3/Gen-4
    dalle.rs        # DALL-E 3 / gpt-image-1
    luma.rs         # Luma Dream Machine
    elevenlabs.rs   # ElevenLabs TTS/SFX
```

Each provider maps the same `MediaBrief` to its own API format:

```rust
// runway.rs — example translation
impl MediaProvider for RunwayProvider {
    async fn translate(&self, brief: &MediaBrief, taste: &str, llm: &LlmClient) -> Result<Translation> {
        let prompt = format!(
            r#"You are translating a creative brief into Runway Gen-3 API parameters.

**Brief:** {intent}
**Why:** {why}
**References:** {refs}

**Taste guide (learned from previous crits):**
{taste}

Produce a JSON object with:
- prompt: the Runway prompt text (be specific about camera, lighting, mood)
- duration: seconds (5 or 10)
- ratio: "16:9", "9:16", or "1:1"
- style: "cinematic", "raw", "analog", etc.
- camera_motion: describe any camera movement

Then defend your translation choices in 2-3 sentences.

Return as:
```json
{{"params": {{...}}, "defense": "..."}}
```"#,
            intent = brief.intent,
            why = brief.why,
            refs = brief.references.join(", "),
            taste = taste,
        );

        let response = llm.call(&prompt).await?;
        // parse and return Translation
    }
}
```

### Task Type Extension

Tasks get a `kind` field:

```rust
pub enum TaskKind {
    Code,       // existing — wright modifies files
    Media,      // new — wright calls a generative API
}
```

Media tasks flow through the same loop but the "work" step calls a provider instead of writing code:

```
Code task:  brief → wright reads files → writes code → defense → crit (diff view)
Media task: brief → wright translates → calls API → defense → crit (preview view)
```

The crit page already has a preview iframe for design tasks. Media tasks use the same pattern — render the video/image inline for the reviewer to evaluate alongside the defense.

### Staging for Media

Media outputs go to staging just like code:

```
.workwright/
  staging/
    site/index.html       # code staging (existing)
    media/                 # media staging (new)
      a3f7c2.mp4          # video output
      a3f7c2.meta.json    # generation params, defense, cost
```

Accept promotes to a media library. Reject discards.

### API Endpoints (additions)

```
POST /api/tasks          # existing — add kind: "media" + media_brief
GET  /api/preview/{id}   # existing — extended to serve media files
GET  /api/providers      # new — list available providers + status
POST /api/estimate       # new — cost estimate before generation
```

### Taste for Media

Media taste signals carry the same structure but the *reasons* are visual/cinematic:

```json
{
  "score": 0.85,
  "reason": "Color grade is right — desaturated, cool dawn light. Camera motion has the weight I want. Duration could be longer — this cut needs room to breathe before the next shot.",
  "task_id": "a3f7c2",
  "tags": ["color", "motion", "pacing"]
}
```

Over time, the taste guide develops sections like:
- **Color:** Prefers desaturated, natural light. Rejects oversaturated AI-default palettes.
- **Motion:** Handheld drift > smooth AI dolly. Weight matters. Nothing should feel weightless.
- **Pacing:** Longer holds. Let the frame settle. Tarkovsky > TikTok.
- **Composition:** Negative space. The empty part of the frame is the composition.

This is a personal visual language — learned from crit, not written by hand.

### Budget Awareness

Media APIs cost real money. The system should:
- Show estimated cost before generation (translation step returns this)
- Track cumulative spend per provider
- Allow budget caps per session/day
- Surface cost in the crit view ("this shot cost $0.50 — was it worth it?")

```rust
pub struct BudgetTracker {
    limits: HashMap<String, f64>,  // provider -> daily cap
    spent: HashMap<String, f64>,   // provider -> spent today
}
```

## What This Enables

### Sequences / Shot Lists

A shot list is just a batch of media briefs with ordering:

```
Shot 1: Wide establishing — fog field at dawn (Runway, 10s)
Shot 2: Close-up — dew on grass blade (DALL-E → Runway img2vid, 5s)
Shot 3: VO — "The land remembers" (ElevenLabs, match to shot 1 duration)
```

Each shot goes through the loop independently. The taste guide ensures visual consistency across shots without manual prompt wrangling.

### Cross-Provider Translation

Same brief, different engines. The intent layer is portable:

```
Brief: "still life of tea on a wooden table, morning light"
  → DALL-E: photorealistic, natural lighting prompt
  → Midjourney: --style raw --ar 16:9 parameters
  → Runway: img2vid from the DALL-E still
```

The wright picks the right provider based on taste + brief + cost.

### A/B Crit

Generate the same brief through two providers. Crit both. The taste signal tells you which engine matches your aesthetic for which kind of shot. This is data no one has right now.

## What to Build First

1. **`ww-media` crate** with the trait + types (no providers yet)
2. **Runway provider** (we already have `runway_gen.py` — port to Rust or shell out)
3. **`kind: media` on tasks** — media briefs accepted through the existing brief flow
4. **Media staging** — files land in `.workwright/staging/media/`
5. **Crit page: media preview** — video/image inline with defense
6. **Cost tracking** — show what each generation costs

Runway first because it's the most interesting (video is harder than images, taste matters more). DALL-E second. Audio third.

## What This Is Not

- Not a prompt library or template system
- Not a Runway wrapper with a nicer UI
- Not batch generation without judgment

It's the same thing Workwright already is — a crit system — pointed at a different material. Code is text that runs. Video is light that moves. The protocol doesn't care. It cares if you can defend your choices.

---

*Sketch — March 15, 2026*
