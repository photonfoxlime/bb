use derive_more::From;
use educe::Educe;
use logos::Logos;
use std::{collections::VecDeque, fmt};

#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(subpattern atomchar = r"[a-zA-Z0-9+*,._~=?!$%&`'<>:;\^\-|/]")]
pub enum Token<'input> {
    // basic symbols
    #[regex(r#"(?&atomchar)+"#)]
    // string literals
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)]
    // excape
    // #[regex(r#"\\@(?&atomchar)*"#)]
    #[regex(r#"\\.(?&atomchar)*"#)]
    Atom(&'input str),

    // line break with trailing whitespaces
    // #[regex(r#"\n\s*"#, priority = 2)]
    // line break after two spaces
    #[regex(r#"\n"#, priority = 2)]
    LineBreak(&'input str),
    // single whitespace
    #[regex(r#"\s"#, priority = 1)]
    WhiteSpace(&'input str),

    #[token("@")]
    At,
    #[token("@@")]
    AtAt,
    #[regex("@@@+", |lex| lex.slice().len())]
    AtAtAt(usize),
    #[token("#")]
    Hash,
    #[token("@end")]
    AtEnd,
    #[token("(")]
    ParenOpen,
    #[token(")")]
    ParenClose,
    #[token("[")]
    BracketOpen,
    #[token("]")]
    BracketClose,
    #[token("{")]
    BraceOpen,
    #[token("}")]
    BraceClose,

    // anything else
    #[regex(".", priority = 0)]
    Unknown(&'input str),
}

impl<'input> fmt::Display for Token<'input> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Atom(s) => write!(f, "{}", s),
            Token::LineBreak(s) => write!(f, "{}", s),
            Token::WhiteSpace(s) => write!(f, "{}", s),
            Token::At => write!(f, "@"),
            Token::AtAt => write!(f, "@@"),
            Token::AtAtAt(n) => write!(f, "{}", "@".repeat(*n)),
            Token::Hash => write!(f, "#"),
            Token::AtEnd => write!(f, "@end"),
            Token::ParenOpen => write!(f, "("),
            Token::ParenClose => write!(f, ")"),
            Token::BracketOpen => write!(f, "["),
            Token::BracketClose => write!(f, "]"),
            Token::BraceOpen => write!(f, "{{"),
            Token::BraceClose => write!(f, "}}"),
            Token::Unknown(s) => write!(f, "{}", s),
        }
    }
}

#[cfg(test)]
mod test_lexer {
    use super::*;

    #[test]
    fn simple() {
        let input = indoc::indoc!(
            r#"
            sdf "asd0duj19~" saen.lk ""x"" \/ |- : ;; @(w [dw wea (x 10.0)])@lck@@a 
                

            #ew(
            @end

            "#
        )
        .trim_start();
        for tok in Token::lexer(input) {
            println!("{:?}", tok.unwrap());
        }
    }

    #[test]
    fn escape() {
        let input = indoc::indoc!(
            r#"
            @code{\@\@atom}
            "#
        )
        .trim_start();
        for tok in Token::lexer(input) {
            println!("{:?}", tok.unwrap());
        }
    }
}

#[derive(From, Clone, Educe)]
#[educe(Debug(named_field = false))]
pub struct Atom {
    pub content: String,
}

#[derive(From, Clone, Debug)]
pub struct Annotation {
    pub delimiter: Delimiter,
    pub elements: Vec<Element>,
}

#[derive(From, Clone, Debug)]
pub enum Element {
    Atom(Atom),
    Annotation(Annotation),
}

#[derive(From, Clone, Debug)]
pub enum Delimiter {
    Paren,
    Bracket,
}

#[derive(From, Clone, Debug)]
pub struct Annotated<Inner> {
    pub attached: VecDeque<Annotation>,
    pub inner: Inner,
}

#[derive(From, Clone, Educe)]
#[educe(Debug(name = true))]
pub enum Entity {
    #[educe(Debug(name = false))]
    Raw(Raw),
    #[educe(Debug(name = true))]
    IncontextAnnotation(Annotation),
    #[educe(Debug(name = false))]
    Item(Annotated<Item>),
    #[educe(Debug(name = false))]
    Blob(Annotated<Blob>),
    #[educe(Debug(name = false))]
    Block(Annotated<Block>),
}

#[derive(From, Clone, Educe)]
#[educe(Debug(named_field = false))]
pub struct Raw {
    pub content: String,
}

#[derive(From, Clone, Debug)]
pub struct Item {
    pub name: Atom,
    pub annotation: Annotation,
}

#[derive(From, Clone, Educe)]
#[educe(Debug(named_field = false))]
pub struct Blob {
    pub content: String,
}

#[derive(From, Clone, Debug)]
pub struct Block {
    pub style: BlockStyle,
    pub name: Atom,
    pub top: Top,
}

#[derive(From, Clone, Debug)]
pub enum BlockStyle {
    Delimited,
    Incontext,
    Braced,
}

#[derive(From, Clone, Debug)]
pub struct Top {
    pub entities: Vec<Entity>,
}

lalrpop_util::lalrpop_mod!(pub grammar);

#[cfg(test)]
mod test_parser {
    use super::*;

    #[test]
    fn annotation() {
        let input = indoc::indoc!(
            r#"
            (w [ dw wea (x 10.0) ])
            "#
        )
        .trim();
        let tokens = Token::lexer(input)
            .spanned()
            .map(|(tok, span)| tok.map(|tok| (span.start, tok, span.end)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let parser = grammar::AnnotationParser::new();
        let result = parser.parse(tokens.into_iter()).unwrap();
        println!("{:#?}", result);
    }

    #[test]
    fn rick_roll() {
        let input = indoc::indoc!(
            r#"
            @@title How did we end up here?
            @block
                @@(never gonna [give (you up)])
                Is there life on Mars?
            @end
            "#
        )
        .trim();
        let tokens = Token::lexer(input)
            .spanned()
            .map(|(tok, span)| tok.map(|tok| (span.start, tok, span.end)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let parser = grammar::TopParser::new();
        let result = parser.parse(tokens.into_iter()).unwrap();
        println!("{:#?}", result);
    }
}
