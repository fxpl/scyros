#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

// Function to add two integers
int add(int a, int b) {
    return a + b;
}

// Function to find the maximum of two floats
float max_float(float a, float b) {
    // Ternary operator
    return (a > b) ? a : b;
}

// Function to calculate the length of a string
size_t string_length(const char *str) {
    return strlen(str);
}

// Function to check if a number is even
int is_even(int num) {
    return num % 2 == 0;
}

// Function to calculate the factorial of a number
unsigned long long factorial(int n) {
    if (n == 0) return 1;
    return n * factorial(n - 1);
}

// Function to swap two integers
void swap(int *a, int *b) {
    int temp = *a;
    *a = *b;
    *b = temp;
}

// Function to reverse a string
void reverse_string(char *str) {
    int len = strlen(str);
    for (int i = 0; i < len / 2; i++) {
        char temp = str[i];
        str[i] = str[len - i - 1];
        str[len - i - 1] = temp;
    }
}

// Function to calculate the power of a number
double power(double base, int exp) {
    return pow(base, exp);
}

// Function to find the minimum of an array of integers
int min_array(int arr[], int size) {
    int min = arr[0];
    for (int i = 1; i < size; i++) {
        if (arr[i] < min) {
            min = arr[i];
        }
    }
    return min;
}

// Function to generate a random number between two values
int random_between(int min, int max) {
    return rand() % (max - min + 1) + min;
}

double tan(double x) {
    if (x == INFINITY) {
        return 0;
    }
    else {
        return sin(x) / cos(x);
    }
}

// Test functions
void test_add() {
    printf("Add: %d\n", add(3, 4));
}

void test_nested_loops() {
    int i, j, k;
    int sum = 0;
    for (i = 0; i < 3; i++) {
        for (j = 0; j < 3; j++) {
            for (k = 0; k < 3; k++) {
                if (i == j) {
                    if (j == k) {
                        while (sum < 10) {
                            sum += i + j + k;
                        }
                    }
                }
            }
        }
    }
    printf("Nested loops sum: %d\n", sum);

    // Additional nested loop example
    int product = 1;
    for (i = 1; i <= 3; i++) {
        for (j = 1; j <= 3; j++) {
            for (k = 1; k <= 3; k++) {
                product *= i * j * k;
            }
        }
    }
    printf("Nested loops product: %d\n", product);
}

void test_max_float() {
    printf("Max float: %.2f\n", max_float(3.5, 4.2));
}

void test_string_length() {
    printf("String length: %zu\n", string_length("Hello"));
}

void test_is_even() {
    printf("Is even: %d\n", is_even(4));
}

void test_factorial() {
    printf("Factorial: %llu\n", factorial(5));
}

void test_swap() {
    int a = 5, b = 10;
    swap(&a, &b);
    printf("Swap: a = %d, b = %d\n", a, b);
}

void test_reverse_string() {
    char str[] = "Hello";
    reverse_string(str);
    printf("Reverse string: %s\n", str);
}

void test_power() {
    printf("Power: %.2f\n", power(2.0, 3));
}

void test_min_array() {
    int arr[] = {3, 1, 4, 1, 5, 9};
    printf("Min array: %d\n", min_array(arr, 6));
}

void test_random_between() {
    printf("Random between 1 and 10: %d\n", random_between(1, 10));
}

int main() {
    test_add();
    test_max_float();
    test_string_length();
    test_is_even();
    test_factorial();
    test_swap();
    test_reverse_string();
    test_power();
    test_min_array();
    test_random_between();
    
    return 0;
}