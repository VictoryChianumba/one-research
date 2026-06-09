/// Map an arXiv category code to a human-readable label.
/// Returns `None` for unmapped codes.
pub fn map_arxiv_category(code: &str) -> Option<&'static str> {
  match code {
    "cs.LG" => Some("machine learning"),
    "cs.AI" => Some("artificial intelligence"),
    "cs.CL" => Some("natural language processing"),
    "cs.CV" => Some("computer vision"),
    "cs.NE" => Some("neural networks"),
    "cs.RO" => Some("robotics"),
    "cs.IR" => Some("information retrieval"),
    "cs.HC" => Some("human-computer interaction"),
    "stat.ML" => Some("machine learning"),
    "stat.TH" => Some("statistics theory"),
    "math.OC" => Some("optimisation"),
    "math.ST" => Some("statistics"),
    "eess.AS" => Some("audio and speech"),
    "eess.IV" => Some("image and video"),
    "eess.SP" => Some("signal processing"),
    "q-bio.NC" => Some("neuroscience"),
    "physics.comp-ph" => Some("computational physics"),
    _ => None,
  }
}

static SUBTOPIC_KEYWORDS: &[(&str, &str)] = &[
  ("sparse autoencoder", "sparse autoencoders"),
  ("mechanistic interp", "mechanistic interpretability"),
  ("circuit", "circuit analysis"),
  ("steering vector", "steering"),
  ("activation patch", "activation patching"),
  ("rlhf", "RLHF"),
  ("reinforcement learning from human", "RLHF"),
  ("rlaif", "RLAIF"),
  ("chain of thought", "chain of thought"),
  ("test-time compute", "test-time compute"),
  ("mixture of experts", "mixture of experts"),
  ("moe", "mixture of experts"),
  ("long context", "long context"),
  ("diffusion", "diffusion models"),
  ("world model", "world models"),
  ("vision language", "vision-language"),
  ("multimodal", "multimodal"),
  ("agent", "agents"),
  ("reasoning", "reasoning"),
  ("alignment", "alignment"),
  ("safety", "safety"),
  ("interpretability", "interpretability"),
  ("transformer", "transformers"),
  ("attention", "attention"),
  ("fine-tun", "fine-tuning"),
  ("distillation", "knowledge distillation"),
  ("quantization", "quantization"),
  ("pruning", "pruning"),
  ("scaling", "scaling"),
  ("benchmark", "benchmarks"),
  ("evaluation", "evaluation"),
];

/// Detect ML subtopics from title and summary text.
/// Matching is case-insensitive. Deduplicates by label.
pub fn detect_subtopics(title: &str, summary: &str) -> Vec<&'static str> {
  let combined = format!("{title} {summary}").to_lowercase();
  let mut topics: Vec<&'static str> = Vec::new();
  for &(keyword, label) in SUBTOPIC_KEYWORDS {
    if combined.contains(keyword) && !topics.iter().any(|&t| t == label) {
      topics.push(label);
    }
  }
  topics
}
