//! Purpose: Allow any user to modify issue labels on GitHub via comments.
//!
//! Labels are checked against the labels in the project; the bot does not support creating new
//! labels.
//!
//! Parsing is done in the `parser::command::label` module.
//!
//! If the command was successful, there will be no feedback beyond the label change to reduce
//! notification noise.

use crate::{
    github::{self, GithubClient},
    interactions::ErrorComment,
    registry::{Event, Handler},
};
use failure::Error;
use parser::command::label::{LabelCommand, LabelDelta};
use parser::command::{Command, Input};

pub struct LabelHandler {
    pub client: GithubClient,
}

impl Handler for LabelHandler {
    fn handle_event(&self, event: &Event) -> Result<(), Error> {
        #[allow(irrefutable_let_patterns)]
        let event = if let Event::IssueComment(e) = event {
            e
        } else {
            // not interested in other events
            return Ok(());
        };

        let mut issue_labels = event.issue.labels().to_owned();

        let mut input = Input::new(&event.comment.body, self.client.username());
        let deltas = match input.parse_command() {
            Command::Label(Ok(LabelCommand(deltas))) => deltas,
            Command::Label(Err(err)) => {
                ErrorComment::new(
                    &event.issue,
                    format!(
                        "Parsing label command in [comment]({}) failed: {}",
                        event.comment.html_url, err
                    ),
                )
                .post(&self.client)?;
                failure::bail!(
                    "label parsing failed for issue #{}, error: {:?}",
                    event.issue.number,
                    err
                );
            }
            _ => return Ok(()),
        };

        let mut changed = false;
        for delta in &deltas {
            let name = delta.label().as_str();
            if let Err(msg) = check_filter(name, &event.comment.user, &self.client) {
                ErrorComment::new(&event.issue, msg).post(&self.client)?;
                return Ok(());
            }
            match delta {
                LabelDelta::Add(label) => {
                    if !issue_labels.iter().any(|l| l.name == label.as_str()) {
                        changed = true;
                        issue_labels.push(github::Label {
                            name: label.to_string(),
                        });
                    }
                }
                LabelDelta::Remove(label) => {
                    if let Some(pos) = issue_labels.iter().position(|l| l.name == label.as_str()) {
                        changed = true;
                        issue_labels.remove(pos);
                    }
                }
            }
        }

        if changed {
            event.issue.set_labels(&self.client, issue_labels)?;
        }

        Ok(())
    }
}

fn check_filter(label: &str, user: &github::User, client: &GithubClient) -> Result<(), String> {
    let is_team_member;
    match user.is_team_member(client) {
        Ok(true) => return Ok(()),
        Ok(false) => {
            is_team_member = Ok(());
        }
        Err(err) => {
            eprintln!("failed to check team membership: {:?}", err);
            is_team_member = Err(());
            // continue on; if we failed to check their membership assume that they are not members.
        }
    }
    if label.starts_with("C-") // categories
    || label.starts_with("A-") // areas
    || label.starts_with("E-") // easy, mentor, etc.
    || label.starts_with("NLL-")
    || label.starts_with("O-") // operating systems
    || label.starts_with("S-") // status labels
    || label.starts_with("T-")
    || label.starts_with("WG-")
    {
        return Ok(());
    }
    match label {
        "I-compilemem" | "I-compiletime" | "I-crash" | "I-hang" | "I-ICE" | "I-slow" => {
            return Ok(());
        }
        _ => {}
    }

    if is_team_member.is_ok() {
        Err(format!(
            "Label {} can only be set by Rust team members",
            label
        ))
    } else {
        Err(format!(
            "Label {} can only be set by Rust team members;\
             we were unable to check if you are a team member.",
            label
        ))
    }
}