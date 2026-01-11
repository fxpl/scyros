// A TypeScript file showcasing various language constructs and exotic features
// related to floating-point arithmetic and operations.
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
var __spreadArray = (this && this.__spreadArray) || function (to, from, pack) {
    if (pack || arguments.length === 2) for (var i = 0, l = from.length, ar; i < l; i++) {
        if (ar || !(i in from)) {
            if (!ar) ar = Array.prototype.slice.call(from, 0, i);
            ar[i] = from[i];
        }
    }
    return to.concat(ar || Array.prototype.slice.call(from));
};
// Enum for different floating-point operations
var FloatOps;
(function (FloatOps) {
    FloatOps["Add"] = "ADD";
    FloatOps["Subtract"] = "SUBTRACT";
    FloatOps["Multiply"] = "MULTIPLY";
    FloatOps["Divide"] = "DIVIDE";
    FloatOps["Modulus"] = "MODULUS";
    FloatOps["Power"] = "POWER";
})(FloatOps || (FloatOps = {}));
// A function that performs a floating-point operation based on the provided enum
function performOperation(op, a, b) {
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
function applyToPairs(operation, numbers) {
    var results = [];
    for (var i = 0; i < numbers.length - 1; i++) {
        results.push(operation(numbers[i], numbers[i + 1]));
    }
    return results;
}
// A recursive function to calculate the sum of sine values of an array of numbers
function recursiveSineSum(numbers, index) {
    if (index === void 0) { index = 0; }
    if (index >= numbers.length) {
        return 0;
    }
    return Math.sin(numbers[index]) + recursiveSineSum(numbers, index + 1);
}
function applyTupleOperations(tuple, operations) {
    return [
        operations[0](tuple[0]),
        operations[1](tuple[1]),
        operations[2](tuple[2]),
    ];
}
// Example of a generator function for floating-point sequences
function floatingPointSequence(start, step, count) {
    var current, i;
    return __generator(this, function (_a) {
        switch (_a.label) {
            case 0:
                current = start;
                i = 0;
                _a.label = 1;
            case 1:
                if (!(i < count)) return [3 /*break*/, 4];
                return [4 /*yield*/, current];
            case 2:
                _a.sent();
                current += step;
                _a.label = 3;
            case 3:
                i++;
                return [3 /*break*/, 1];
            case 4: return [2 /*return*/];
        }
    });
}
// Example of using a Proxy to intercept floating-point operations
var floatHandler = {
    get: function (target, prop) {
        if (prop in target) {
            return target[prop];
        }
        throw new Error("Property ".concat(prop, " does not exist"));
    },
    set: function (target, prop, value) {
        if (typeof value === "number") {
            target[prop] = value;
            return true;
        }
        throw new Error("value: ".concat(value));
    },
};
var floatObject = new Proxy({ x: 0, y: 0 }, floatHandler);
// Example usage
var numbers = [1.1, 2.2, 3.3, 4.4];
var results = applyToPairs(function (a, b) { return a + b; }, numbers);
console.log("Pairwise sums:", results);
var sineSum = recursiveSineSum(numbers);
console.log("Recursive sine sum:", sineSum);
var tuple = [1.5, 2.5, 3.5];
var sequence = floatingPointSequence(0.5, 0.5, 5);
console.log("Floating-point sequence:", __spreadArray([], sequence, true));
floatObject.x = 42.42;
console.log("Float object x:", floatObject.x);
