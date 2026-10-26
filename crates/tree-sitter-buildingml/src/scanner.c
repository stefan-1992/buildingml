// BuildingML external scanner for tree-sitter
// Handles NEWLINE, INDENT, DEDENT tokens for indentation-sensitive blocks.
// Pure C for WASM compatibility.
//
// Design: at each line boundary, emit exactly one of:
//   INDENT  — indent increased (absorbs the newline)
//   DEDENT  — indent decreased (may emit multiple)
//   NEWLINE — same indent level
//
// This means INDENT implies "newline + indent increase".

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// #define DEBUG_SCANNER 1
#ifdef DEBUG_SCANNER
#define DPRINTF(...) fprintf(stderr, __VA_ARGS__)
#else
#define DPRINTF(...)
#endif

enum TokenType {
    NEWLINE,
    INDENT,
    DEDENT,
};

#define MAX_INDENT_DEPTH 64

typedef struct {
    uint16_t indent_stack[MAX_INDENT_DEPTH];
    uint8_t stack_size;
    // Number of pending DEDENT tokens still to emit
    uint8_t pending_dedents;
} Scanner;

static void scanner_init(Scanner *scanner) {
    scanner->stack_size = 1;
    scanner->indent_stack[0] = 0;
    scanner->pending_dedents = 0;
}

static uint16_t scanner_current_indent(Scanner *scanner) {
    if (scanner->stack_size == 0) return 0;
    return scanner->indent_stack[scanner->stack_size - 1];
}

void *tree_sitter_buildingml_external_scanner_create(void) {
    Scanner *scanner = (Scanner *)calloc(1, sizeof(Scanner));
    scanner_init(scanner);
    return scanner;
}

void tree_sitter_buildingml_external_scanner_destroy(void *payload) {
    free(payload);
}

unsigned tree_sitter_buildingml_external_scanner_serialize(
    void *payload, char *buffer
) {
    Scanner *scanner = (Scanner *)payload;
    unsigned size = 0;
    buffer[size++] = (char)scanner->stack_size;
    buffer[size++] = (char)scanner->pending_dedents;
    for (uint8_t i = 0; i < scanner->stack_size; i++) {
        buffer[size++] = (char)(scanner->indent_stack[i] & 0xFF);
        buffer[size++] = (char)((scanner->indent_stack[i] >> 8) & 0xFF);
    }
    return size;
}

void tree_sitter_buildingml_external_scanner_deserialize(
    void *payload, const char *buffer, unsigned length
) {
    Scanner *scanner = (Scanner *)payload;
    if (length == 0) {
        scanner_init(scanner);
        return;
    }
    unsigned pos = 0;
    scanner->stack_size = (uint8_t)buffer[pos++];
    scanner->pending_dedents = (uint8_t)buffer[pos++];
    for (uint8_t i = 0; i < scanner->stack_size && pos + 1 < length; i++) {
        scanner->indent_stack[i] =
            (uint16_t)((unsigned char)buffer[pos]) |
            ((uint16_t)((unsigned char)buffer[pos + 1]) << 8);
        pos += 2;
    }
}

bool tree_sitter_buildingml_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols
) {
    Scanner *scanner = (Scanner *)payload;

    // Emit pending DEDENTs first (unconditional — see comment on indent < current)
    if (scanner->pending_dedents > 0) {
        scanner->pending_dedents--;
        lexer->result_symbol = DEDENT;
        return true;
    }

    // At EOF, only emit remaining DEDENTs (never NEWLINE — avoids infinite loop)
    if (lexer->eof(lexer)) {
        if (valid_symbols[DEDENT] && scanner->stack_size > 1) {
            scanner->stack_size--;
            lexer->result_symbol = DEDENT;
            return true;
        }
        return false;
    }

    // Only process at newline characters
    if (lexer->lookahead != '\n') {
        return false;
    }

    // Consume the newline
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);

    // Skip blank lines and count indent of the next content line
    uint16_t indent = 0;
    while (!lexer->eof(lexer)) {
        if (lexer->lookahead == ' ') {
            indent++;
            lexer->advance(lexer, false);
        } else if (lexer->lookahead == '\t') {
            // Treat tabs as 2 spaces (but spec says spaces only)
            indent += 2;
            lexer->advance(lexer, false);
        } else if (lexer->lookahead == '\r') {
            lexer->advance(lexer, false);
        } else if (lexer->lookahead == '\n') {
            // Blank line — emit NEWLINE for this blank line, reset indent
            lexer->advance(lexer, false);
            lexer->mark_end(lexer);
            indent = 0;
        } else {
            // Found content
            break;
        }
    }

    if (lexer->eof(lexer)) {
        // File ends after we consumed a \n.
        // We must emit NEWLINE to terminate the current line's rule (heading,
        // property, etc.), but this is safe because on the NEXT call we enter
        // via the EOF handler at the top which only emits DEDENTs.
        if (valid_symbols[NEWLINE]) {
            lexer->result_symbol = NEWLINE;
            return true;
        }
        if (valid_symbols[DEDENT] && scanner->stack_size > 1) {
            scanner->stack_size--;
            lexer->result_symbol = DEDENT;
            return true;
        }
        return false;
    }

    uint16_t current = scanner_current_indent(scanner);

    DPRINTF("SCANNER: indent=%d current=%d stack_size=%d valid=[NL=%d IN=%d DE=%d]\n",
            indent, current, scanner->stack_size,
            valid_symbols[NEWLINE], valid_symbols[INDENT], valid_symbols[DEDENT]);

    if (indent > current) {
        // Increased indent
        if (valid_symbols[INDENT]) {
            if (scanner->stack_size < MAX_INDENT_DEPTH) {
                scanner->indent_stack[scanner->stack_size++] = indent;
            }
            lexer->mark_end(lexer);
            lexer->result_symbol = INDENT;
            return true;
        }
        // If INDENT not valid, try NEWLINE
        if (valid_symbols[NEWLINE]) {
            lexer->result_symbol = NEWLINE;
            return true;
        }
        return false;
    }

    if (indent < current) {
        // Decreased indent — emit NEWLINE first (to terminate the current line's
        // rule like simple_property), then queue DEDENTs as pending.
        uint8_t dedent_count = 0;
        while (scanner->stack_size > 1 &&
               scanner->indent_stack[scanner->stack_size - 1] > indent) {
            scanner->stack_size--;
            dedent_count++;
        }

        if (dedent_count > 0) {
            scanner->pending_dedents = dedent_count;
        }

        // Emit NEWLINE to terminate the current line. DEDENTs will follow
        // as pending tokens on subsequent scanner calls.
        if (valid_symbols[NEWLINE]) {
            lexer->result_symbol = NEWLINE;
            return true;
        }

        // If NEWLINE isn't valid, emit the first DEDENT directly
        if (scanner->pending_dedents > 0) {
            scanner->pending_dedents--;
            lexer->result_symbol = DEDENT;
            return true;
        }
        return false;
    }

    // Same indent level
    if (valid_symbols[NEWLINE]) {
        lexer->result_symbol = NEWLINE;
        return true;
    }

    return false;
}
