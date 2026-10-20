use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alternative {
    pub value: String,
    pub score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Finding {
    pub name: String,
    pub value: String,
    pub confidence: Option<f64>,
    pub alternatives: Vec<Alternative>,
    pub limitations: Vec<String>,
    pub unknowns: Vec<String>,
}

impl Finding {
    // a finding with only its name and value, to fill in the rest
    pub fn new(name: &str, value: String) -> Finding {
        Finding {
            name: name.to_string(),
            value,
            confidence: None,
            alternatives: Vec::new(),
            limitations: Vec::new(),
            unknowns: Vec::new(),
        }
    }

    // the finding as a few readable lines, value and confidence first
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![match self.confidence {
            Some(c) => format!("{}: {} (confidence {:.2})", self.name, self.value, c),
            None => format!("{}: {}", self.name, self.value),
        }];
        if !self.alternatives.is_empty() {
            let others: Vec<String> = self
                .alternatives
                .iter()
                .map(|a| format!("{} ({:.2})", a.value, a.score))
                .collect();
            out.push(format!("  also possible: {}", others.join(", ")));
        }
        for unknown in &self.unknowns {
            out.push(format!("  unknown: {}", unknown));
        }
        for limitation in &self.limitations {
            out.push(format!("  limitation: {}", limitation));
        }
        out
    }
}

// how sure a choice is from its score and the best other one, 0 for a tie and 1 when nothing else came close
pub fn margin(best: f64, runner_up: f64) -> f64 {
    if best <= 0.0 {
        return 0.0;
    }
    round(1.0 - runner_up / best)
}

// rounds to 2 decimals and keeps the value between 0 and 1
pub fn round(value: f64) -> f64 {
    ((value * 100.0).round() / 100.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // a tie is not sure at all, a lone choice is fully sure, and the lines show alternatives and unknowns
    #[test]
    fn margins_and_lines() {
        assert_eq!(margin(1.0, 1.0), 0.0);
        assert_eq!(margin(1.0, 0.0), 1.0);
        assert_eq!(margin(0.8, 0.6), 0.25);
        let mut finding = Finding::new("tempo", "120 bpm".to_string());
        finding.confidence = Some(0.25);
        finding.alternatives.push(Alternative {
            value: "240 bpm".to_string(),
            score: 0.6,
        });
        finding.unknowns.push("half or double time".to_string());
        let lines = finding.lines();
        assert_eq!(lines[0], "tempo: 120 bpm (confidence 0.25)");
        assert_eq!(lines[1], "  also possible: 240 bpm (0.60)");
        assert_eq!(lines[2], "  unknown: half or double time");
    }
}
