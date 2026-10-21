use crate::flp::events::{self, Event, Project, Value};

struct Insert {
    flags: usize,
    slots_end: usize,
}

// where each mixer insert's flags event is and where its effect slots end, the master first
fn inserts(project: &Project) -> Vec<Insert> {
    let mut out: Vec<Insert> = Vec::new();
    for (i, event) in project.events.iter().enumerate() {
        match event.id {
            events::INSERT_FLAGS => out.push(Insert {
                flags: i,
                slots_end: i + 1,
            }),
            events::INSERT_ROUTING => {
                if let Some(last) = out.last_mut() {
                    if last.slots_end == last.flags + 1 {
                        last.slots_end = i;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// replaces the effect slots of one insert with a copy of another project's insert, plugins and settings included
pub fn copy_effects(project: &mut Project, target: usize, from: &Project, source: usize) -> bool {
    let theirs = inserts(from);
    let ours = inserts(project);
    let (Some(source), Some(target)) = (theirs.get(source), ours.get(target)) else {
        return false;
    };
    if source.slots_end <= source.flags + 1 || target.slots_end <= target.flags + 1 {
        return false;
    }
    let slots: Vec<Event> = from.events[source.flags + 1..source.slots_end].to_vec();
    project
        .events
        .splice(target.flags + 1..target.slots_end, slots);
    true
}

// the insert a channel of a project is routed to
pub fn channel_insert(project: &Project, start: usize, end: usize) -> Option<usize> {
    project.events[start..end]
        .iter()
        .find_map(|e| match (e.id, &e.value) {
            (events::CHANNEL_INSERT, Value::Byte(insert)) => Some(*insert as usize),
            _ => None,
        })
}

// gives a mixer insert other than the master a name, shown at the top of its strip in fl studio
pub fn name_insert(project: &mut Project, target: usize, name: &str) {
    let all = inserts(project);
    let (Some(insert), Some(before)) = (
        all.get(target),
        target.checked_sub(1).and_then(|t| all.get(t)),
    ) else {
        return;
    };
    let from = before.slots_end;
    let text = Value::Data(events::encode_text(name));
    match (from..insert.flags).find(|&i| project.events[i].id == events::INSERT_NAME) {
        Some(i) => project.events[i].value = text,
        None => project.events.insert(
            insert.flags,
            Event {
                id: events::INSERT_NAME,
                value: text,
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // a project with a master and two inserts, the second holding one effect
    fn mixer(effect: Option<&str>) -> Project {
        let mut events = Vec::new();
        for n in 0..3 {
            events.push(Event {
                id: events::INSERT_FLAGS,
                value: Value::Data(vec![0; 12]),
            });
            if n == 2 {
                if let Some(name) = effect {
                    events.push(Event {
                        id: events::PLUGIN_NAME,
                        value: Value::Data(events::encode_text(name)),
                    });
                }
            }
            for slot in 0..10 {
                events.push(Event {
                    id: events::SLOT_INDEX,
                    value: Value::Word(slot),
                });
            }
            events.push(Event {
                id: events::INSERT_ROUTING,
                value: Value::Data(vec![0; 4]),
            });
        }
        Project {
            format: 0,
            channel_count: 0,
            ppq: 96,
            events,
        }
    }

    // an effect copied onto insert one shows up there and the insert can be named
    #[test]
    fn copies_effects() {
        let from = mixer(Some("Fruity Reeverb 2"));
        let mut project = mixer(None);
        assert!(copy_effects(&mut project, 1, &from, 2));
        name_insert(&mut project, 1, "Break");
        let summary = crate::flp::info::summarize(&project);
        assert_eq!(summary.inserts[1].effects, vec!["Fruity Reeverb 2"]);
        assert_eq!(summary.inserts[1].name.as_deref(), Some("Break"));
        assert!(summary.inserts[2].effects.is_empty());
    }
}
