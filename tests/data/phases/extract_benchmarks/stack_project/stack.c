// stack.c
// Implementation of stack
// Written by Bowen Wu on 05/09/2022
// ============================================================================

#include "stack.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ============================================================================
// Implementations
// ============================================================================

Stack* stack_new(void)
{
    Stack* new_stack = malloc(sizeof(Stack));
    new_stack->head = NULL;
    return new_stack;
}

void stack_free(Stack* self)
{
    Node* current = self->head->next;
    Node* prev = self->head;
    while (current != NULL) {
        prev = current;
        current = current->next;
        node_free(prev);
    }

    free(self);
}

Node* node_new(char data)
{
    Node* new_node = malloc(sizeof(Node));
    new_node->data = data;
    new_node->next = NULL;
    return new_node;
}

void node_free(Node* self)
{
    free(self);
}

void stack_push(Stack* self, char data)
{
    Node* new_node = node_new(data);

    if (stack_is_empty(self))
    {
        self->head = new_node;
    }
    else
    {
        new_node->next = self->head;
        self->head = new_node;
    }
}

char stack_pop(Stack* self)
{
    if (stack_is_empty(self))
    {
        return '\0';
    }

    char data;
    data = self->head->data;

    Node* temp = self->head;
    self->head = self->head->next;
    node_free(temp);

    return data;
}

void stack_print(Stack* self)
{
    if (stack_is_empty(self)) {
        return;
    }

    Node* current = self->head;
    while (current != NULL) {
        printf("%c", current->data);
        current = current->next;
    }
}

bool stack_is_empty(Stack* self)
{
    return self->head == NULL;
}

int is_balanced(Stack* self, char* formula)
{
    int index = 0;

    // Returns -1 on success
    // Returns the index of the failed bracket

    // Check each character in the formula
    while (formula[index] != 0) {

        // Is it an opening bracket
        if (formula[index] == '{')
            stack_push(self, '{');
        if (formula[index] == '[')
            stack_push(self, '[');
        if (formula[index] == '(')
            stack_push(self, '(');

        // Is it a closing bracket and does it match
        if ((formula[index] == '}') && (stack_pop(self) != '{'))
            return index;
        if ((formula[index] == ']') && (stack_pop(self) != '['))
            return index;
        if ((formula[index] == ')') && (stack_pop(self) != '('))
            return index;

        index++;
    }

    // Check to see if the stack is empty
    if (stack_is_empty(self)) {
        return -1;
    } else {
        return index;
    }
}

// ============================================================================
// Main
// ============================================================================

int main()
{

    char formula[64];

    // Create the stack
    Stack* stack = stack_new();

    // Get a formula from the user
    printf("Formula: ");
    fgets(formula, 64, stdin);
    formula[strcspn(formula, "\r\n")] = 0;

    // Is the formula balanced
    int index = is_balanced(stack, formula);

    // Print the result
    if (index == -1) {
        printf("Formula is balanced\n");
    } else {
        printf("Formula is NOT balanced\n");
        printf("%s\n", formula);
        for (int i = 0; i < index; i++)
            printf(" ");
        printf("^\n");
    }
}
