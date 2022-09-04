use colored::Colorize;
use docopt::Docopt;
use openmls::prelude::TlsSerializeTrait;

use crate::{error::NodeError, node::Node};

// Write the Docopt usage string.
const USAGE: &str = "
Usage: node create
       node join
       node send <message>
";

type Message = Vec<u8>;

// Command line helper for Node actions
pub fn parse_stdin(node: &mut Node, line: String) -> Result<Message, NodeError> {
    let args_res = Docopt::new(USAGE).and_then(|d| d.argv(line.split(' ')).parse());
    let mut msg = Vec::new();
    match args_res {
        Ok(args) => {
            let user_message = args.get_str("<message>");
            if args.get_bool("create") {
                println!("Creating new group.");
                node.join_new_group();
            } else if args.get_bool("join") {
                println!("Joining group.");
                msg = node
                    .get_key_package()
                    .tls_serialize_detached()
                    .expect("key should serialize");
            } else if !user_message.is_empty() {
                msg = node
                    .create_message(user_message)?
                    .tls_serialize_detached()
                    .expect("message should serialize");
                println!("{}: {}", "me".to_string().red(), user_message);
            }
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    Ok(msg)
}
