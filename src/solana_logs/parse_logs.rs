use log::error;
use regex::Regex;

use super::EventListenerError;

struct Execution {
    stack: Vec<String>,
}

impl Execution {
    fn program(&self) -> Result<String, EventListenerError> {
        if self.stack.is_empty() {
            error!("Failed to get program from the empty stack");
            return Err(EventListenerError::SolanaParseLogs);
        }
        Ok(self.stack[self.stack.len() - 1].clone())
    }

    fn push(&mut self, new_program: String) {
        self.stack.push(new_program);
    }

    fn pop(&mut self) -> Result<(), EventListenerError> {
        if self.stack.is_empty() {
            error!("Failed to get program from the empty stack");
            return Err(EventListenerError::SolanaParseLogs);
        }
        self.stack.pop().expect("Stack should not be empty");
        Ok(())
    }

    fn update(&mut self, log: &str) -> Result<String, EventListenerError> {
        let re =
            Regex::new(r"^Program (.*) invoke.*$").expect("Expected regexp to be constructed well");
        let Some(c) = re.captures(log) else {
            return self.program();
        };
        let program = c
            .get(1)
            .expect("Expected captured program address to be available")
            .as_str()
            .to_string();
        self.push(program);
        self.program()
    }
}

pub(crate) fn parse_logs<T: anchor_lang::Event + anchor_lang::AnchorDeserialize>(
    logs: &[&str],
    program_id_str: &str,
) -> Result<Vec<T>, EventListenerError> {
    let mut events: Vec<T> = Vec::new();
    let mut do_pop = false;
    if !logs.is_empty() {
        let mut execution = Execution {
            stack: <Vec<String>>::default(),
        };
        for log in logs {
            let (event, pop) = {
                if do_pop {
                    execution.pop()?;
                }
                execution.update(log)?;
                if program_id_str == execution.program()? {
                    handle_program_log(program_id_str, log).map_err(|e| {
                        error!("Failed to parse log: {}", e);
                        EventListenerError::SolanaParseLogs
                    })?
                } else {
                    let (_, pop) = handle_irrelevant_log(program_id_str, log);
                    (None, pop)
                }
            };
            do_pop = pop;
            if let Some(e) = event {
                events.push(e);
            }
        }
    }
    Ok(events)
}

fn handle_program_log<T: anchor_lang::Event + anchor_lang::AnchorDeserialize>(
    self_program_str: &str,
    l: &str,
) -> Result<(Option<T>, bool), EventListenerError> {
    const PROGRAM_LOG: &str = "Program log: ";
    const PROGRAM_DATA: &str = "Program data: ";

    if let Some(log) = l.strip_prefix(PROGRAM_LOG).or_else(|| l.strip_prefix(PROGRAM_DATA)) {
        let borsh_bytes = match anchor_lang::__private::base64::decode(log) {
            Ok(borsh_bytes) => borsh_bytes,
            _ => {
                #[cfg(feature = "debug")]
                println!("Could not base64 decode log: {}", log);
                return Ok((None, false));
            }
        };

        let mut slice: &[u8] = &borsh_bytes[..];
        let disc: [u8; 8] = {
            let mut disc = [0; 8];
            disc.copy_from_slice(&borsh_bytes[..8]);
            slice = &slice[8..];
            disc
        };
        let mut event = None;
        if disc == T::discriminator() {
            let e: T = anchor_lang::AnchorDeserialize::deserialize(&mut slice).map_err(|err| {
                error!("Failed to deserialize event: {}", err);
                EventListenerError::SolanaParseLogs
            })?;
            event = Some(e);
        }
        Ok((event, false))
    } else {
        let (_program, did_pop) = handle_irrelevant_log(self_program_str, l);
        Ok((None, did_pop))
    }
}

fn handle_irrelevant_log(this_program_str: &str, log: &str) -> (Option<String>, bool) {
    let re = Regex::new(r"^Program (.*) invoke$")
        .expect("Expected invoke regexp to be constructed well");
    if log.starts_with(&format!("Program {this_program_str} log:")) {
        (Some(this_program_str.to_string()), false)
    } else if let Some(c) = re.captures(log) {
        (
            c.get(1)
                .expect("Expected the captured program to be available")
                .as_str()
                .to_string()
                .into(),
            false,
        )
    } else {
        let re =
            Regex::new(r"^Program (.*) success$").expect("Expected regexp to be constructed well");
        (None, re.is_match(log))
    }
}

#[cfg(test)]
mod test {
    use crate::common::solana_logs::parse_logs;
    use photon::{ProposeEvent, ID as PROGRAM_ID};

    #[test]
    fn test_logs_parsing() {
        static SAMPLE: &[&str] = &[
            "Program EjpcUpcuJV2Mq9vjELMZHhgpvJ4ggoWtUYCTFqw6D9CZ invoke [1]",
            "Program log: Instruction: ShareMessage",
            "Program log: Share message invoked",
            "Program 3cAFEXstVzff2dXH8PFMgm81h8sQgpdskFGZqqoDgQkJ invoke [2]",
            "Program log: Instruction: Propose",
            "Program data: 8vb9LnW1kqUgAAAAb25lZnVuY19fX19fX19fX19fX19fX19fX19fX19fX18IAAAAAAAAAG2BAAAAAAAAAAAAAAAAAAADAAAAAQIDAwAAAAECAwMAAAABAgM=",
            "Program 3cAFEXstVzff2dXH8PFMgm81h8sQgpdskFGZqqoDgQkJ consumed 16408 of 181429 compute units",
            "Program 3cAFEXstVzff2dXH8PFMgm81h8sQgpdskFGZqqoDgQkJ success",
            "Program EjpcUpcuJV2Mq9vjELMZHhgpvJ4ggoWtUYCTFqw6D9CZ consumed 35308 of 200000 compute units",
            "Program EjpcUpcuJV2Mq9vjELMZHhgpvJ4ggoWtUYCTFqw6D9CZ success",
        ];

        let events: Vec<ProposeEvent> = parse_logs::parse_logs(SAMPLE, &PROGRAM_ID.to_string())
            .expect("Processing logs should not result in errors");
        assert_eq!(events.len(), 1);
        let propose_event = events.first().expect("No events caught");
        assert_eq!(propose_event.dst_chain_id, 33133);
        assert_eq!(propose_event.params, vec![1, 2, 3]);
        assert_eq!(propose_event.protocol_id.as_slice(), b"onefunc_________________________");
    }

    #[test]
    fn test_deploy_programs() {
        static SAMPLE: &[&str] = &[
            "Program 11111111111111111111111111111111 invoke [1]",
            "Program 11111111111111111111111111111111 success",
            "Program BPFLoaderUpgradeab1e11111111111111111111111 invoke [1]",
            "Program 11111111111111111111111111111111 invoke [2]",
            "Program 11111111111111111111111111111111 success",
            "Deployed program 3cAFEXstVzff2dXH8PFMgm81h8sQgpdskFGZqqoDgQkJ",
            "Program BPFLoaderUpgradeab1e11111111111111111111111 success",
        ];
        let events: Vec<ProposeEvent> = parse_logs::parse_logs(SAMPLE, &PROGRAM_ID.to_string())
            .expect("Processing logs should not result in errors");
        assert!(events.is_empty(), "Expected no events have been met")
    }
}
