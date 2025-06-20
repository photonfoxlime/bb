# Markup with More Structures

Consider a markup language that subsumes multiple DSLs, with each DSL definable by users in a composable and context-sensitive way. We compare this markup language with other popular languages like markdown, xml, and org in terms of expressivity, extensivity, and ergonomics.

## Yet another markup language, why?

Let's start by limiting the scope of this blog. What do we use markup languages for? To write documents with enriched format options, of course. That's how all the markup languages like md, xml, and org started, and that's what they've achieved. But that's not the end of the story.

Challenges come from org, one of the most complicated and ambitious languages I've ever seen. Literate mode empowers org to work as a configuration language with arbitrarily many auxiliary documentation. Embeddable code blocks effectively turns org into Jupiter Notebook. As if it's not enough, the ability to add metadata in the header or even in the document makes it super easy, despite being a little hacky, to customize the behavior of the document itself in a dramatically flexible way, especially in the ecosystem of elisp. The closer we look at such a versatile markup language, the more we find it similar to a programming language. We want more functionality even with the cost of writing more code that is not directly visible to the reader.

But that's not yet the full picture. Being one of the most popular choices of note taking and planning, Notion features block-based documents that allows structural organization of user content. Combined with the `@` prompted completions, Notion greatly improves the quality of life with the IDE-like experience. Famous markdown-based editor Obsidian practices the idea of backlinks to form a graph of notes, proving that some sort of meaningful structure is desirable by the user.

Finally, a user-friendly markup language should be text-based. It's definitely easier maintain and serialize compared to the binary format, and being text-based also makes the language open and welcoming. As a user, we want to see what's going on underneath and edit the file directly when we feel like it.

Programable, structural, and text-based. This is what we're trying to achieve here.

### What makes a good markup language?

Traditionally, a good markup language should be attractive to both readers and writers, who might be tomorrow version of you and yesterday version of you, simultaneously. As a result, the language should be easy to type in, with less character edits or editor actions for the same content. At the same time, the language should also look familiar to how it's rendered, or at least give a hint about its appearance to the reader at the writing time.

...

In conclusion, text-based markup languages can be evaluated under four criteria:

1. Minimal effort to typeset, potentially with the help of an IDE
2. Visual similarity to how it should be rendered to the reader
3. Semantic clarity of structural content only by looking at the syntax
4. Programmability for more functionality and rich interaction

In the ideal case, the language should possess all four merits. However, these merits doesn't naturally come up together in the design space of markup language grammar. Existing languages have made their choice, in my personal perspective, as follows:

| Language |         Editing         |          Visually Alike           |            Structured             |      Programmable       |
| :------: | :---------------------: | :-------------------------------: | :-------------------------------: | :---------------------: |
| Markdown | :ballot_box_with_check: |      :ballot_box_with_check:      |            :thinking:             |        :warning:        |
|   XML    |        :warning:        | :thinking::ballot_box_with_check: |      :ballot_box_with_check:      | :ballot_box_with_check: |
|   Org    | :ballot_box_with_check: |      :ballot_box_with_check:      |            :thinking:             | :ballot_box_with_check: |
|  Notion  | :ballot_box_with_check: |          :no_entry_sign:          |      :ballot_box_with_check:      |       :thinking:        |
| Obsidian | :ballot_box_with_check: |      :ballot_box_with_check:      | :thinking::ballot_box_with_check: |        :warning:        |
|  Ours?   | :ballot_box_with_check: | :thinking::ballot_box_with_check: |      :ballot_box_with_check:      | :ballot_box_with_check: |

:no_entry_sign: - not applicable​

:warning: - concerning​

:thinking: - questionable​

:thinking::ballot_box_with_check: - reasonable after all

:ballot_box_with_check: - fairly good​

Focus too much on visual similarity will significantly complicate the parser and limit potential extensions to the language, which is the case for markdown and org.

Implementing everything from scratch is not wise, because reinventing wheels is a difficult yet meaningless task. We prefer to use whatever is already there, possibly via pandoc.

## What's your idea then?

Our goal can actually be broken down into two steps:

1. Make a text-based structural language by carefully design its syntax, and then
2. Provide programmability through semantic actions.

And to summarize the solution to each step, structures are represented in a concept of "blocks", and the semantic actions are delegated to external programs by manipulating context-included ASTs, serialized into json, sexp, lua, or any feasible data representation.

The following draft is more of a reflection on what elements should the idealized markup language possess than a concrete specification, meaning all details can be adjusted or further customized. However, it can definitely be considered as a referential guide for designing a markup language.

### Surface Syntax and Core Syntax

The syntax overall is designed to be very concise. In the following paragraphs, we start from the surface syntax and talk about core syntax through elaboration. Apart from a few symbols and tokens that act as delimiters to form structures throughout a repository of all documents, the acceptable input is rather arbitrary.

An atom, similar to atoms in lisp languages, are basic identifiers. The actual design decision is rather flexible. Personally, I don't refuse emojis or string literals, but not white spaces or the escape character `\`. We denote atoms as `atom` in the paragraphs to come. Apart from atoms, whitespaces need to be distinguished during lexical analysis as well.

Blocks are how the language organizes structures. Call it "list" if you're a lisp fan. The symbol `@`, followed immediately by an atom, is a block delimiter. A pair of `@atom` and `@end` forms a block that can contain or live in other blocks, along with other arbitrary text contents. The core syntax may be designed to wrap all uncontained text segments into (`@text`, `@end`) blocks. The whole document is implicitly wrapped by a user-specified block in core syntax, such as (`@text`, `@end`) or (`@md`, `@end`).

A pair of (`@block`, `@end`) delimiters does nothing other than structural organization, so it's called a parentheses block.

Some blocks that can fit into one line can also be written in the following sequence: `@atom`, white spaces, `{`, inner content, and `}`. The curly brace syntax can be desugared into normal block delimiters in core syntax. The formatter is suggested to change curly braces back to `@end` on a line warp.

If some "consecutive" content can be included in a block, one may choose to write `@@atom`, delimited by a single line break `\n` instead of `@end` to denote a block, which will be desugared in an obvious way in core syntax. This is called an in-context block, meaning that the content is somewhat inlined. An example use case is a list item in markdown. Since a list should not contain paragraphs that divides the list (which makes the whole list visually confusing), we use `\n` as a delimiter.

Blob blocks parses inner content separately. Delimited by `@@@` and `@@@`, any content inside will be handled as is. If `@@@` is included in the content, just add more `@`s to the delimiters.

Enough for the blocks. Time for annotations and items.

The syntax for annotation `@(lisp)` is a symbol `@` immediately followed by a well-delimited `(` `)` s-expression, with atoms as terminals inside. The `(lisp)` above simply represents an s-expression. Semantically, annotations are attached to the block immediately after itself. A block may have any number of annotations, forming a list whose order is determined by the order of appearance of annotations for this block in the document.

An in-context annotation `@@(lisp)` is an annotation attached to the surrounding block. The motivation is to allow more flexible annotation that have more comments in context. It can be trivially desugared.

(Do we really need this? It introduces an ambiguity.) A block may contain also annotations itself as in `@atom(lisp)` and `@end(lisp)`, desugaring to `@(lisp)@atom` and `@@(lisp)@end`, respectively.

Apart from using `(` and `)` as s-expression delimiters, `[` and `]` should also be considered. In this language, `[` and `]` are used for collections of homogeneous data (not quite), while `(` and `)` are for heterogeneous ones. A typical example is representing an array/tuple `[1 0]` v.s. a map/anonymous record `[(x 1) (y 0)]` v.s. a named record `(vec (x 1) (y 0))` v.s. a named tuple `(vec 1 0)`. The motivation is that when using vanilla s-expressions to represent metadata, I found some reasonable critics about their appearance and visual attractiveness. Personally, I think the most important issue among all is not being able to tell the intension of the author in a glance, mostly due to the language's over-simplicity. However, the actual semantics of `[` and `]` should be further reconciled since the underneath rationale is not satisfying so far. Maybe `(name ...)` should always contain a `name` at the beginning to give the whole expression a theme, while `[...]` means it's all about data. Anyways, from now on, we denote an s-expression involving both delimiters as `(lisp)`. And a series of `@(lisp)@(lisp)...` is equivalent to `@[(lisp) (lisp)]...`.

Not all structural content should have children. Those that shouldn't can be represented as items `#atom(lisp)` (or `#atom`, short for `#atom()`). Items can be used to represent links, references (backlinks), or even inclusion of other documents. To encode how the link is rendered (e.g. as url text, as an image, as a pdf, or even as a webpage), the lisp-formatted attributes provides extra expressivity. Items can also be annotated. Blocks and items are collectively called entities.

Finally, the escapes. The only necessary escapes throughout the document are `\\`, `\@`, `\#`, `\{`, and `\}`.

#### Case Studies

A table sublanguage, implemented by markdown, may look like the following:

```
@table
| a | b | c |
| - | - | - |
| ? | ! | ~ |
@end
```

which, unsurprisingly, can be rendered as:

| a    | b    | c    |
| ---- | ---- | ---- |
| ?    | !    | ~    |

As mentioned, links are accompanied with how it's supposed to be rendered. An image is encoded as `#link(image url)`, where a `url` is a string literal or raw string literal and if it's a file path, it can be either absolute or relative. Besides `image`, `pdf` and `web` are other available options. Text links, despite most frequently used, are not `#link` items. Instead, they should be treated as normal blocks containing text (or actually anything), but annotated with `@(link url)`. With the link annotation, the whole block will be "clickable" when being hovered upon, and by clicking the user jumps to the url.

#### Layout

todo: design a layout language for blocks; explain indentation and trims

The content inside a block, or the inner content of a block, is suggested to be indented, and the indentation will be automatically removed following the method shown in the [indoc](https://docs.rs/indoc/latest/indoc/) crate. Inner contents are trimmed by default. If needed, whitespaces can be manually added with `#space` items.

#### Bindings and Namespaces

`@(ref name)` gives the annotated entity (either block or item) a `name` that can be referred to elsewhere. `#link(ref name)` is such a reference. A block may have multiple names and multiple references at the same time. The scope of these names remains to be further designed. Roughly speaking, the scope could be at document level.

`@(use path paths)`

`@(unuse)`

An atom path is a sequence of atoms separated by `/`. All `atom` tokens mentioned above can be upgraded to be `path` without losing generality.

#### Annotation Macros

`@(@foo a b)` 

"compile-time functions" that provide abstractions over annotations

### Semantics

The semantics of blocks is defined by a set of rewrite rules; or rather, an equational theory.

Text blocks can be separated and merged as long as the order is preserved. Atoms and white spaces are preferred units of each text block, but the semantics should not be affected by any decision of text separation.

Semantic actions are defined by shell invocation to external programs.