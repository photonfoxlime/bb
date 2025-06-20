use bb::{Token, grammar::TopParser};
use logos::Logos;
use std::io::{self, Read, Write};

fn main() {
    loop {
        // prompt
        println!("Enter input (Ctrl+D to finish):");
        // read input
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .expect("Failed to read line");
        let input = String::from_utf8(buf).expect("Failed to parse input");
        // lex
        let tokens = Token::lexer(&input)
            .spanned()
            .map(|(tok, span)| tok.map(|tok| (span.start, tok, span.end)))
            .collect::<Result<Vec<_>, _>>();
        match tokens {
            Ok(tokens) => {
                // parse
                let parser = TopParser::new();
                match parser.parse(tokens.into_iter()) {
                    Ok(result) => println!("{:#?}", result),
                    Err(e) => println!("Parsing error: {:?}", e),
                }
            }
            Err(e) => println!("Lexing error: {:?}", e),
        }
        // continue?
        print!("Continue? [Y/n] ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");
        if input.is_empty() || input.trim() == "n" {
            break;
        }
    }
}
