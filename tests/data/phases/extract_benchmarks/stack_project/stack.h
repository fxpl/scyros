// stack.h
// Header for stack
// Written by Bowen Wu on 05/09/2022
// ============================================================================

// ============================================================================
// Includes
// ============================================================================

#pragma once

#include <stdbool.h>

// ============================================================================
// Structures
// ============================================================================

typedef struct node_t {
    char data;
    struct node_t* next;
} Node;

typedef struct stack_t {
    struct node_t* head;
} Stack;

// ============================================================================
// Prototypes
// ============================================================================

Stack* stack_new(void);

void stack_free(Stack* self);

Node* node_new(char data);

void node_free(Node* node);

void stack_push(Stack* self, char data);

char stack_pop(Stack* self);

bool stack_is_empty(Stack* self);

int is_balanced(Stack* self, char* formula);
