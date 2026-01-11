using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace ExoticFloatingPoint
{
    public static class FloatingPointPlayground
    {
        // Static readonly field
        private static readonly double Epsilon = 1e-10;

        // Tuple return type
        public static (double Sin, double Cos) ComputeSinCos(double angle)
        {
            return (Math.Sin(angle), Math.Cos(angle));
        }

        // Local function
        public static double Hypotenuse(double a, double b)
        {
            double Square(double x) => x * x;
            return Math.Sqrt(Square(a) + Square(b));
        }

        // Recursive function
        public static double RecursivePower(double baseValue, int exponent)
        {
            if (exponent == 0) return 1;
            if (exponent < 0) return 1 / RecursivePower(baseValue, -exponent);
            return baseValue * RecursivePower(baseValue, exponent - 1);
        }

        // LINQ with floating-point computations
        public static double AverageOfSquares(IEnumerable<double> numbers)
        {
            return numbers.Select(x => x * x).Average();
        }

        // Async method with floating-point computation
        public static async Task<double> ComputePiAsync(int terms)
        {
            return await Task.Run(() =>
            {
                double pi = 0;
                for (int k = 0; k < terms; k++)
                {
                    pi += Math.Pow(-1, k) / (2 * k + 1);
                }
                return 4 * pi;
            });
        }

        // Operator overloading for a custom floating-point struct
        public struct ExoticFloat
        {
            public double Value { get; }

            public ExoticFloat(double value)
            {
                Value = value;
            }

            public static ExoticFloat operator +(ExoticFloat a, ExoticFloat b) => new ExoticFloat(a.Value + b.Value);
            public static ExoticFloat operator *(ExoticFloat a, ExoticFloat b) => new ExoticFloat(a.Value * b.Value);

            public override string ToString() => Value.ToString("F4");
        }


        // Pattern matching with floating-point ranges
        public static string CategorizeNumber(double number) => number switch
        {
            < 0 => "Negative",
            >= 0 and < 1 => "Small",
            >= 1 and < 100 => "Medium",
            >= 100 => "Large",
            _ => "Unknown"
        };

        // Extension method for floating-point arrays
        public static double StandardDeviation(this IEnumerable<double> numbers)
        {
            var mean = numbers.Average();
            var variance = numbers.Select(x => Math.Pow(x - mean, 2)).Average();
            return Math.Sqrt(variance);
        }

        // Main method to demonstrate features
        public static void Main()
        {
            Console.WriteLine("Sin(π/4), Cos(π/4): " + ComputeSinCos(Math.PI / 4));
            Console.WriteLine("Hypotenuse(3, 4): " + Hypotenuse(3, 4));
            Console.WriteLine("2^10: " + RecursivePower(2, 10));
            Console.WriteLine("Average of squares: " + AverageOfSquares(new[] { 1.0, 2.0, 3.0 }));
            Console.WriteLine("Fast inverse square root of 4: " + FastInverseSquareRoot(4));
            Console.WriteLine("Categorize 42: " + CategorizeNumber(42));
            Console.WriteLine("Standard deviation: " + new[] { 1.0, 2.0, 3.0 }.StandardDeviation());

            var exotic1 = new ExoticFloat(3.14);
            var exotic2 = new ExoticFloat(2.71);
            Console.WriteLine("ExoticFloat addition: " + (exotic1 + exotic2));
            Console.WriteLine("ExoticFloat multiplication: " + (exotic1 * exotic2));

            var piTask = ComputePiAsync(1000000);
            Console.WriteLine("Computed Pi: " + piTask.Result);
        }
    }
}