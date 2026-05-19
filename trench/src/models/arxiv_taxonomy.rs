//! Static arXiv subject taxonomy.
//!
//! Three-level hierarchy mirroring arxiv.org's category browser:
//!     Group   → e.g. "Computer Science"
//!     Archive → e.g. "Computing Research Repository" (cs)
//!     Category → e.g. "Machine Learning" (cs.LG)
//!
//! Source: https://arxiv.org/category_taxonomy (canonical list).
//!
//! The Subject Browser (ADR-005) navigates this table; the curated
//! general feed (`config.sources.arxiv_categories`) is the subset of
//! Category.code values the user has promoted via the `p` gesture.
//!
//! This module is deliberately decoupled from `models/categories.rs`.
//! `map_arxiv_category` is the hot-path label dictionary used per-row
//! when rendering feed items; this table is read once per frame in the
//! Browser. Conflating them would couple a render seam to a config seam.

/// A single arXiv category — the leaf of the taxonomy.
/// `code` is the canonical arXiv form (e.g. `cs.LG`, `physics.optics`,
/// or a bare archive id like `gr-qc` for archives without subcategories).
pub struct Category {
  pub code: &'static str,
  pub name: &'static str,
}

/// An arXiv archive — one or more categories sharing a submission stream.
/// `id` is the URL-stable archive identifier (e.g. `cs`, `astro-ph`).
/// `id` is read by ADR-010 PR 2's worker when building per-archive
/// fetch URLs and by ADR-010 PR 3's promotion gesture for config keys.
pub struct Archive {
  #[allow(dead_code)] // PR 2 (worker URL builder)
  pub id: &'static str,
  pub name: &'static str,
  pub categories: &'static [Category],
}

/// A top-level arXiv group — the column headers on arxiv.org's home page.
/// `id` is read by ADR-010 PR 2 when emitting per-group telemetry.
pub struct Group {
  #[allow(dead_code)] // PR 2 (worker URL builder)
  pub id: &'static str,
  pub name: &'static str,
  pub archives: &'static [Archive],
}

/// Look up a category by its canonical code (e.g. `cs.LG`). Linear scan
/// over ~155 entries — microseconds, no need for a phf map. Consumed by
/// the Subject column renderer (ADR-011 §E5) to identify which entries
/// in `FeedItem.domain_tags` are canonical category codes vs detected
/// subtopics / human labels.
pub fn find_category(code: &str) -> Option<&'static Category> {
  for g in TAXONOMY {
    for a in g.archives {
      for c in a.categories {
        if c.code == code {
          return Some(c);
        }
      }
    }
  }
  None
}

/// Iterate every category across every group. Used by tripwire K1 to
/// assert the table size and by the Browser to compute column-4 counts.
pub fn all_categories() -> impl Iterator<Item = &'static Category> {
  TAXONOMY.iter().flat_map(|g| g.archives.iter().flat_map(|a| a.categories.iter()))
}

/// Total number of top-level groups. Locked by tripwire K1 at 8 — arXiv
/// hasn't added a new top-level subject area in over a decade, so a
/// change here is almost certainly a typo / accidental delete.
pub fn group_count() -> usize {
  TAXONOMY.len()
}

// ---------------------------------------------------------------------------
// Taxonomy data
// ---------------------------------------------------------------------------

pub const TAXONOMY: &[Group] = &[
  Group {
    id: "physics",
    name: "Physics",
    archives: &[
      Archive {
        id: "astro-ph",
        name: "Astrophysics",
        categories: &[
          Category { code: "astro-ph.CO", name: "Cosmology and Nongalactic Astrophysics" },
          Category { code: "astro-ph.EP", name: "Earth and Planetary Astrophysics" },
          Category { code: "astro-ph.GA", name: "Astrophysics of Galaxies" },
          Category { code: "astro-ph.HE", name: "High Energy Astrophysical Phenomena" },
          Category { code: "astro-ph.IM", name: "Instrumentation and Methods for Astrophysics" },
          Category { code: "astro-ph.SR", name: "Solar and Stellar Astrophysics" },
        ],
      },
      Archive {
        id: "cond-mat",
        name: "Condensed Matter",
        categories: &[
          Category { code: "cond-mat.dis-nn", name: "Disordered Systems and Neural Networks" },
          Category { code: "cond-mat.mes-hall", name: "Mesoscale and Nanoscale Physics" },
          Category { code: "cond-mat.mtrl-sci", name: "Materials Science" },
          Category { code: "cond-mat.other", name: "Other Condensed Matter" },
          Category { code: "cond-mat.quant-gas", name: "Quantum Gases" },
          Category { code: "cond-mat.soft", name: "Soft Condensed Matter" },
          Category { code: "cond-mat.stat-mech", name: "Statistical Mechanics" },
          Category { code: "cond-mat.str-el", name: "Strongly Correlated Electrons" },
          Category { code: "cond-mat.supr-con", name: "Superconductivity" },
        ],
      },
      Archive {
        id: "gr-qc",
        name: "General Relativity and Quantum Cosmology",
        categories: &[Category { code: "gr-qc", name: "General Relativity and Quantum Cosmology" }],
      },
      Archive {
        id: "hep-ex",
        name: "High Energy Physics — Experiment",
        categories: &[Category { code: "hep-ex", name: "High Energy Physics — Experiment" }],
      },
      Archive {
        id: "hep-lat",
        name: "High Energy Physics — Lattice",
        categories: &[Category { code: "hep-lat", name: "High Energy Physics — Lattice" }],
      },
      Archive {
        id: "hep-ph",
        name: "High Energy Physics — Phenomenology",
        categories: &[Category { code: "hep-ph", name: "High Energy Physics — Phenomenology" }],
      },
      Archive {
        id: "hep-th",
        name: "High Energy Physics — Theory",
        categories: &[Category { code: "hep-th", name: "High Energy Physics — Theory" }],
      },
      Archive {
        id: "math-ph",
        name: "Mathematical Physics",
        categories: &[Category { code: "math-ph", name: "Mathematical Physics" }],
      },
      Archive {
        id: "nlin",
        name: "Nonlinear Sciences",
        categories: &[
          Category { code: "nlin.AO", name: "Adaptation and Self-Organizing Systems" },
          Category { code: "nlin.CD", name: "Chaotic Dynamics" },
          Category { code: "nlin.CG", name: "Cellular Automata and Lattice Gases" },
          Category { code: "nlin.PS", name: "Pattern Formation and Solitons" },
          Category { code: "nlin.SI", name: "Exactly Solvable and Integrable Systems" },
        ],
      },
      Archive {
        id: "nucl-ex",
        name: "Nuclear Experiment",
        categories: &[Category { code: "nucl-ex", name: "Nuclear Experiment" }],
      },
      Archive {
        id: "nucl-th",
        name: "Nuclear Theory",
        categories: &[Category { code: "nucl-th", name: "Nuclear Theory" }],
      },
      Archive {
        id: "physics",
        name: "Physics (other)",
        categories: &[
          Category { code: "physics.acc-ph", name: "Accelerator Physics" },
          Category { code: "physics.ao-ph", name: "Atmospheric and Oceanic Physics" },
          Category { code: "physics.app-ph", name: "Applied Physics" },
          Category { code: "physics.atm-clus", name: "Atomic and Molecular Clusters" },
          Category { code: "physics.atom-ph", name: "Atomic Physics" },
          Category { code: "physics.bio-ph", name: "Biological Physics" },
          Category { code: "physics.chem-ph", name: "Chemical Physics" },
          Category { code: "physics.class-ph", name: "Classical Physics" },
          Category { code: "physics.comp-ph", name: "Computational Physics" },
          Category { code: "physics.data-an", name: "Data Analysis, Statistics and Probability" },
          Category { code: "physics.ed-ph", name: "Physics Education" },
          Category { code: "physics.flu-dyn", name: "Fluid Dynamics" },
          Category { code: "physics.gen-ph", name: "General Physics" },
          Category { code: "physics.geo-ph", name: "Geophysics" },
          Category { code: "physics.hist-ph", name: "History and Philosophy of Physics" },
          Category { code: "physics.ins-det", name: "Instrumentation and Detectors" },
          Category { code: "physics.med-ph", name: "Medical Physics" },
          Category { code: "physics.optics", name: "Optics" },
          Category { code: "physics.plasm-ph", name: "Plasma Physics" },
          Category { code: "physics.pop-ph", name: "Popular Physics" },
          Category { code: "physics.soc-ph", name: "Physics and Society" },
          Category { code: "physics.space-ph", name: "Space Physics" },
        ],
      },
      Archive {
        id: "quant-ph",
        name: "Quantum Physics",
        categories: &[Category { code: "quant-ph", name: "Quantum Physics" }],
      },
    ],
  },
  Group {
    id: "math",
    name: "Mathematics",
    archives: &[Archive {
      id: "math",
      name: "Mathematics",
      categories: &[
        Category { code: "math.AC", name: "Commutative Algebra" },
        Category { code: "math.AG", name: "Algebraic Geometry" },
        Category { code: "math.AP", name: "Analysis of PDEs" },
        Category { code: "math.AT", name: "Algebraic Topology" },
        Category { code: "math.CA", name: "Classical Analysis and ODEs" },
        Category { code: "math.CO", name: "Combinatorics" },
        Category { code: "math.CT", name: "Category Theory" },
        Category { code: "math.CV", name: "Complex Variables" },
        Category { code: "math.DG", name: "Differential Geometry" },
        Category { code: "math.DS", name: "Dynamical Systems" },
        Category { code: "math.FA", name: "Functional Analysis" },
        Category { code: "math.GM", name: "General Mathematics" },
        Category { code: "math.GN", name: "General Topology" },
        Category { code: "math.GR", name: "Group Theory" },
        Category { code: "math.GT", name: "Geometric Topology" },
        Category { code: "math.HO", name: "History and Overview" },
        Category { code: "math.IT", name: "Information Theory" },
        Category { code: "math.KT", name: "K-Theory and Homology" },
        Category { code: "math.LO", name: "Logic" },
        Category { code: "math.MG", name: "Metric Geometry" },
        Category { code: "math.MP", name: "Mathematical Physics" },
        Category { code: "math.NA", name: "Numerical Analysis" },
        Category { code: "math.NT", name: "Number Theory" },
        Category { code: "math.OA", name: "Operator Algebras" },
        Category { code: "math.OC", name: "Optimization and Control" },
        Category { code: "math.PR", name: "Probability" },
        Category { code: "math.QA", name: "Quantum Algebra" },
        Category { code: "math.RA", name: "Rings and Algebras" },
        Category { code: "math.RT", name: "Representation Theory" },
        Category { code: "math.SG", name: "Symplectic Geometry" },
        Category { code: "math.SP", name: "Spectral Theory" },
        Category { code: "math.ST", name: "Statistics Theory" },
      ],
    }],
  },
  Group {
    id: "cs",
    name: "Computer Science",
    archives: &[Archive {
      id: "cs",
      name: "Computing Research Repository",
      categories: &[
        Category { code: "cs.AI", name: "Artificial Intelligence" },
        Category { code: "cs.AR", name: "Hardware Architecture" },
        Category { code: "cs.CC", name: "Computational Complexity" },
        Category { code: "cs.CE", name: "Computational Engineering, Finance, and Science" },
        Category { code: "cs.CG", name: "Computational Geometry" },
        Category { code: "cs.CL", name: "Computation and Language" },
        Category { code: "cs.CR", name: "Cryptography and Security" },
        Category { code: "cs.CV", name: "Computer Vision and Pattern Recognition" },
        Category { code: "cs.CY", name: "Computers and Society" },
        Category { code: "cs.DB", name: "Databases" },
        Category { code: "cs.DC", name: "Distributed, Parallel, and Cluster Computing" },
        Category { code: "cs.DL", name: "Digital Libraries" },
        Category { code: "cs.DM", name: "Discrete Mathematics" },
        Category { code: "cs.DS", name: "Data Structures and Algorithms" },
        Category { code: "cs.ET", name: "Emerging Technologies" },
        Category { code: "cs.FL", name: "Formal Languages and Automata Theory" },
        Category { code: "cs.GL", name: "General Literature" },
        Category { code: "cs.GR", name: "Graphics" },
        Category { code: "cs.GT", name: "Computer Science and Game Theory" },
        Category { code: "cs.HC", name: "Human-Computer Interaction" },
        Category { code: "cs.IR", name: "Information Retrieval" },
        Category { code: "cs.IT", name: "Information Theory" },
        Category { code: "cs.LG", name: "Machine Learning" },
        Category { code: "cs.LO", name: "Logic in Computer Science" },
        Category { code: "cs.MA", name: "Multiagent Systems" },
        Category { code: "cs.MM", name: "Multimedia" },
        Category { code: "cs.MS", name: "Mathematical Software" },
        Category { code: "cs.NA", name: "Numerical Analysis" },
        Category { code: "cs.NE", name: "Neural and Evolutionary Computing" },
        Category { code: "cs.NI", name: "Networking and Internet Architecture" },
        Category { code: "cs.OH", name: "Other Computer Science" },
        Category { code: "cs.OS", name: "Operating Systems" },
        Category { code: "cs.PF", name: "Performance" },
        Category { code: "cs.PL", name: "Programming Languages" },
        Category { code: "cs.RO", name: "Robotics" },
        Category { code: "cs.SC", name: "Symbolic Computation" },
        Category { code: "cs.SD", name: "Sound" },
        Category { code: "cs.SE", name: "Software Engineering" },
        Category { code: "cs.SI", name: "Social and Information Networks" },
        Category { code: "cs.SY", name: "Systems and Control" },
      ],
    }],
  },
  Group {
    id: "q-bio",
    name: "Quantitative Biology",
    archives: &[Archive {
      id: "q-bio",
      name: "Quantitative Biology",
      categories: &[
        Category { code: "q-bio.BM", name: "Biomolecules" },
        Category { code: "q-bio.CB", name: "Cell Behavior" },
        Category { code: "q-bio.GN", name: "Genomics" },
        Category { code: "q-bio.MN", name: "Molecular Networks" },
        Category { code: "q-bio.NC", name: "Neurons and Cognition" },
        Category { code: "q-bio.OT", name: "Other Quantitative Biology" },
        Category { code: "q-bio.PE", name: "Populations and Evolution" },
        Category { code: "q-bio.QM", name: "Quantitative Methods" },
        Category { code: "q-bio.SC", name: "Subcellular Processes" },
        Category { code: "q-bio.TO", name: "Tissues and Organs" },
      ],
    }],
  },
  Group {
    id: "q-fin",
    name: "Quantitative Finance",
    archives: &[Archive {
      id: "q-fin",
      name: "Quantitative Finance",
      categories: &[
        Category { code: "q-fin.CP", name: "Computational Finance" },
        Category { code: "q-fin.EC", name: "Economics" },
        Category { code: "q-fin.GN", name: "General Finance" },
        Category { code: "q-fin.MF", name: "Mathematical Finance" },
        Category { code: "q-fin.PM", name: "Portfolio Management" },
        Category { code: "q-fin.PR", name: "Pricing of Securities" },
        Category { code: "q-fin.RM", name: "Risk Management" },
        Category { code: "q-fin.ST", name: "Statistical Finance" },
        Category { code: "q-fin.TR", name: "Trading and Market Microstructure" },
      ],
    }],
  },
  Group {
    id: "stat",
    name: "Statistics",
    archives: &[Archive {
      id: "stat",
      name: "Statistics",
      categories: &[
        Category { code: "stat.AP", name: "Applications" },
        Category { code: "stat.CO", name: "Computation" },
        Category { code: "stat.ME", name: "Methodology" },
        Category { code: "stat.ML", name: "Machine Learning" },
        Category { code: "stat.OT", name: "Other Statistics" },
        Category { code: "stat.TH", name: "Statistics Theory" },
      ],
    }],
  },
  Group {
    id: "eess",
    name: "Electrical Engineering and Systems Science",
    archives: &[Archive {
      id: "eess",
      name: "Electrical Engineering and Systems Science",
      categories: &[
        Category { code: "eess.AS", name: "Audio and Speech Processing" },
        Category { code: "eess.IV", name: "Image and Video Processing" },
        Category { code: "eess.SP", name: "Signal Processing" },
        Category { code: "eess.SY", name: "Systems and Control" },
      ],
    }],
  },
  Group {
    id: "econ",
    name: "Economics",
    archives: &[Archive {
      id: "econ",
      name: "Economics",
      categories: &[
        Category { code: "econ.EM", name: "Econometrics" },
        Category { code: "econ.GN", name: "General Economics" },
        Category { code: "econ.TH", name: "Theoretical Economics" },
      ],
    }],
  },
];

#[cfg(test)]
mod tests {
  use super::*;

  // K1 tripwire pre-condition: the static table has exactly 8 groups
  // and ~155 categories. arXiv hasn't added a new top-level group in
  // over a decade; an off-by-one here is almost certainly an accidental
  // edit, not a real schema change. These tests anchor PR 3's
  // `scripts/check-subject-browser.sh` K1 invariant.

  #[test]
  fn taxonomy_has_eight_groups() {
    assert_eq!(group_count(), 8, "arXiv has 8 top-level groups");
  }

  #[test]
  fn taxonomy_category_count_matches_arxiv() {
    // 155 is the current arxiv.org/category_taxonomy count as of 2026-05.
    // If this number drifts, refresh from the canonical source and
    // update K1 in scripts/check-subject-browser.sh in lockstep.
    assert_eq!(all_categories().count(), 155);
  }

  #[test]
  fn find_category_round_trips() {
    let c = find_category("cs.LG").expect("cs.LG is in the table");
    assert_eq!(c.code, "cs.LG");
    assert_eq!(c.name, "Machine Learning");
  }

  #[test]
  fn find_category_returns_none_for_unknown() {
    assert!(find_category("not.a.real.code").is_none());
    assert!(find_category("").is_none());
  }

  #[test]
  fn every_category_code_is_unique() {
    // Duplicate codes would silently shadow each other in find_category
    // and double-count in all_categories — catch a copy-paste error in
    // the static table at test time, not in production.
    let mut codes: Vec<&str> = all_categories().map(|c| c.code).collect();
    codes.sort_unstable();
    let n = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), n, "category codes must be unique across the table");
  }
}
