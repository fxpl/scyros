// A TypeScript file showcasing various language constructs and exotic features
// related to floating-point arithmetic and operations.

type FloatOperation = (a: number, b: number) => number;

// Enum for different floating-point operations
enum FloatOps {
    Add = "ADD",
    Subtract = "SUBTRACT",
    Multiply = "MULTIPLY",
    Divide = "DIVIDE",
    Modulus = "MODULUS",
    Power = "POWER",
}

// A union type for possible results of floating-point operations
type FloatResult = number | "NaN" | "Infinity";

// A function that performs a floating-point operation based on the provided enum
function performOperation(op: FloatOps, a: number, b: number): FloatResult {
    switch (op) {
        case FloatOps.Add:
            return a + b;
        case FloatOps.Subtract:
            return a - b;
        case FloatOps.Multiply:
            return a * b;
        case FloatOps.Divide:
            return b !== 0 ? a / b : "Infinity";
        case FloatOps.Modulus:
            return b !== 0 ? a % b : "NaN";
        case FloatOps.Power:
            return Math.pow(a, b);
        default:
            throw new Error("Unsupported operation");
    }
}

// A higher-order function that takes a FloatOperation and applies it to an array of numbers
function applyToPairs(
    operation: FloatOperation,
    numbers: number[]
): number[] {
    const results: number[] = [];
    for (let i = 0; i < numbers.length - 1; i++) {
        results.push(operation(numbers[i], numbers[i + 1]));
    }
    return results;
}

// A recursive function to calculate the sum of sine values of an array of numbers
function recursiveSineSum(numbers: number[], index = 0): number {
    if (index >= numbers.length) {
        return 0;
    }
    return Math.sin(numbers[index]) + recursiveSineSum(numbers, index + 1);
}

// A function using exotic types like tuples and mapped types
type FloatTuple = [number, number, number];
type FloatTupleOperations = {
    [K in keyof FloatTuple]: (value: number) => number;
};

function applyTupleOperations(
    tuple: FloatTuple,
    operations: FloatTupleOperations
): FloatTuple {
    return [
        operations[0](tuple[0]),
        operations[1](tuple[1]),
        operations[2](tuple[2]),
    ];
}

// Example of a generator function for floating-point sequences
function* floatingPointSequence(start: number, step: number, count: number) {
    let current = start;
    for (let i = 0; i < count; i++) {
        yield current;
        current += step;
    }
}

// Example of using a Proxy to intercept floating-point operations
const floatHandler = {
    get(target: any, prop: string) {
        if (prop in target) {
            return target[prop];
        }
        throw new Error(`Property ${prop} does not exist`);
    },
    set(target: any, prop: string, value: any) {
        if (typeof value === "number") {
            target[prop] = value;
            return true;
        }
        throw new Error(`value: ${value}`);
    },
};

// Interface for a floating-point calculator
interface FloatCalculator {
    performOperation: (op: FloatOps, a: number, b: number) => FloatResult;
    applyToPairs: (operation: FloatOperation, numbers: number[]) => number[];
    recursiveSineSum: (numbers: number[], index?: number) => number;
    applyTupleOperations: (
        tuple: FloatTuple,
        operations: FloatTupleOperations
    ) => FloatTuple;
    floatingPointSequence: (
        start: number,
        step: number,
        count: number
    ) => Generator<number, void, unknown>;
}

const floatObject = new Proxy({ x: 0, y: 0 }, floatHandler);

// Example usage
const numbers = [1.1, 2.2, 3.3, 4.4];
const results = applyToPairs((a, b) => a + b, numbers);
console.log("Pairwise sums:", results);

const sineSum = recursiveSineSum(numbers);
console.log("Recursive sine sum:", sineSum);

const tuple: FloatTuple = [1.5, 2.5, 3.5];

const sequence = floatingPointSequence(0.5, 0.5, 5);
console.log("Floating-point sequence:", [...sequence]);

floatObject.x = 42.42;
console.log("Float object x:", floatObject.x);