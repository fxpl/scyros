#include <iostream>
#include <cmath>
#include <limits>
#include <iomanip>
#include <type_traits>

/* Macro definition */
#define PI 3.14159265358979323846

// Template function
template<typename T>
T square(T x) {
    static_assert(std::is_floating_point<T>::value, "Template parameter must be a float or double");
    return x * x;
}

// Namespace
namespace MathUtils {
    // Inline function
    inline float cube(float x) {
        return x * x * x;
    }

    // Function pointer
    typedef double (*MathFunc)(double);

    // Lambda function
    auto sqrtLambda = [](double x) -> double {
        return std::sqrt(x);
    };
}

// Enum class
enum class RoundingMode {
    UP,
    DOWN,
    NEAREST
};

// Function with default arguments
double roundToNearest(double value, RoundingMode mode = RoundingMode::NEAREST) {
    switch (mode) {
        case RoundingMode::UP:
            return std::ceil(value);
        case RoundingMode::DOWN:
            return std::floor(value);
        case RoundingMode::NEAREST:
        default:
            return std::round(value);
    }
}

// Variadic template function
template<typename... Args>
double sum(Args... args) {
    return (args + ...);
}

// Struct with member function
struct FloatPrinter {
    void print(float value) const {
        std::cout << "Float value: " << value << std::endl;
    }
};

// Union
union FloatIntUnion {
    float f;
    int i;
};

// Function with exception handling
void checkInfinity(float value) {
    if (std::isinf(value)) {
        throw std::overflow_error("Value is infinity");
    }
}

int main() {
    try {
        // Floating point literals
        float a = true ? 1.23f : 0.0f; // Ternary operator
        double b = 4.56;
        long double c = 7.89L;

        // Using macro
        double circumference = 2 * PI * b;

        // Using template function
        double bSquared = square(b);

        // Using namespace and inline function
        float aCubed = MathUtils::cube(a);

        // Using function pointer
        MathUtils::MathFunc sqrtFunc = MathUtils::sqrtLambda;
        double sqrtB = sqrtFunc(b);

        // Using enum class and function with default arguments
        double roundedValue = roundToNearest(circumference, RoundingMode::UP);

        // Using variadic template function
        double totalSum = sum(a, b, c);

        // Using struct and member function
        FloatPrinter printer;
        printer.print(a);

        // Using union
        FloatIntUnion fiUnion;
        fiUnion.f = a;
        std::cout << "Union int representation of float: " << fiUnion.i << std::endl;

        // Using exception handling
        checkInfinity(std::numeric_limits<float>::infinity());

    } catch (const std::exception& e) {
        std::cerr << "Exception: " << e.what() << std::endl;
    }

    return 0;
}

double IntegrationOfFunctions::calculate_trapezoid_integral(const Vector<double>& x, const Vector<double>& y)
{
   

   int n = x.get_size();

   

   double trapezoid_integral = 0;

   for(int i = 0; i < n-1; i++)
   {
      trapezoid_integral += 0.5*(x[i+1]-x[i])*(y[i+1]+y[i]);
   }

   

   return(trapezoid_integral);
}