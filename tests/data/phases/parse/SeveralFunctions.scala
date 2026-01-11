import scala.math._

trait FloatOps {
    def compute(x: Double): Double
    def description: String = "Performs float operations"
}

abstract class AbstractFloatProcessor {
    def process(values: Seq[Double]): Double
    def name: String
}

class ConcreteFloatProcessor extends AbstractFloatProcessor with FloatOps {
    override def process(values: Seq[Double]): Double = {
        var sum = 0.0
        // Handle NaN values by treating them as zero
        for (v <- values) {
            sum += (if (v.isNaN) 0.0 else v)
        }
        sum / (if (values.isEmpty) 1 else values.size)
    }

    override def compute(x: Double): Double = {
        x match {
            case Double.PositiveInfinity => 0.0
            case Double.NegativeInfinity => 0.0
            case d if d.isNaN            => -1.0
            case d if d > 0              => sqrt(d) + log(d)
            case d if d < 0              => abs(d) * sin(d)
            case _                       => 0.0
        }
    }

    override def name: String = "ConcreteFloatProcessor"
}

object FloatUtils {
    def factorial(n: Int): Double = {
        var result = 1.0
        var i = 1
        while (i <= n) {
            result *= i
            i += 1
        }
        result
    }

    def sumUntilEpsilon(start: Double, epsilon: Double): Double = {
        var sum = 0.0
        var term = start
        /**
         * This loop continues to add terms until the absolute value of the term is less than epsilon.
         * The term is halved each iteration, simulating a converging series.
         */ 
        do {
            sum += term
            term /= 2.0
        } while (abs(term) > epsilon)
        sum
    }

    def findFirstNegative(xs: Seq[Double]): Option[Double] = {
        xs.find(_ < 0)
    }

    def transcendentalOps(x: Double): Double = {
        exp(x) + cos(x) - tanh(x)
    }

    def specialValuesDemo(): Seq[Double] = {
        Seq(Double.NaN, Double.PositiveInfinity, Double.NegativeInfinity, Double.MinValue, Double.MaxValue, 0.0, -0.0)
    }
}

// Example usage
object Main {
    def main(args: Array[String]): Unit = {
        val processor = new ConcreteFloatProcessor
        val data = Seq(1.0, 2.0, Double.NaN, -3.0, Double.PositiveInfinity)
        println(s"Processed: ${processor.process(data)}")
        println(s"Compute(4.0): ${processor.compute(4.0)}")
        println(s"Factorial(5): ${FloatUtils.factorial(5)}")
        println(s"Sum until epsilon: ${FloatUtils.sumUntilEpsilon(1.0, 1e-5)}")
        println(s"First negative: ${FloatUtils.findFirstNegative(data)}")
        println(s"Transcendental ops: ${FloatUtils.transcendentalOps(Pi)}")
        println(s"Special values: ${FloatUtils.specialValuesDemo().mkString(", ")}")
    }
}