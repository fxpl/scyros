import java.util.Arrays;
import java.util.List;

package data;


interface FloatOperations {
    float add(float a, float b);
    float subtract(float a, float b);
    float multiply(float a, float b);
    float divide(float a, float b) throws ArithmeticException;
}

abstract class AbstractFloatOperations implements FloatOperations {
    @Override
    public float add(float a, float b) {
        return a + b;
    }

    @Override
    public float subtract(float a, float b) {
        return a - b;
    }
}

class BasicFloatOperations extends AbstractFloatOperations {
    @Override
    public float multiply(float a, float b) {
        return a * b;
    }

    @Override
    public float divide(float a, float b) throws ArithmeticException {
        if (b == 0) {
            throw new ArithmeticException("Division by zero");
        }
        return a / b;
    }
}

public class SeveralFunctions {
    public static void main(String[] args) {
        BasicFloatOperations operations = new BasicFloatOperations();
        List<Float> numbers = Arrays.asList(10.5f, 2.0f, 0.0f);

        for (float number : numbers) {
            try {
                System.out.println("Addition: " + operations.add(number, 5.5f));
                System.out.println("Subtraction: " + operations.subtract(number, 1.5f));
                System.out.println("Multiplication: " + operations.multiply(number, 3.0f));
                System.out.println("Division: " + operations.divide(number, 2.0f));
            } catch (ArithmeticException e) {
                System.out.println("Error: " + e.getMessage());
            }
        }

        float testValue = numbers.isEmpty() ? 0.1f : 3.5f;
        switch (Float.compare(testValue, 3.5f)) {
            case 0:
                String message = switch (Float.compare(testValue, 3.5f)) {
                    case 0 -> "Equal to 3.5";
                    case 1 -> "Greater than 3.5";
                    case -1 -> "Less than 3.5";
                    default -> "Unexpected comparison result";
                };
                System.out.println(message);
                System.out.println("The value is exactly 3.5");
                break;
            case 1:
                System.out.println("The value is greater than 3.5");
                break;
            case -1:
                System.out.println("The value is less than 3.5");
                break;
            default:
                System.out.println("Unexpected comparison result");
        }
    }
}