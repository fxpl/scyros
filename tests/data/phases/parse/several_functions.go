package main

import (
	"fmt"
	"math"
	"math/cmplx"
	"runtime"
)

// Constants with floating-point values
const (
	Pi       = math.Pi
	Epsilon  = 1e-9
	Golden   = (1 + math.Sqrt(5)) / 2
)

// Function with variadic floating-point arguments
func sumFloats(nums ...float64) float64 {
	sum := 0.0
	for _, num := range nums {
		sum += num
	}
	return sum
}

// Function returning multiple floating-point values
func polarToCartesian(r, theta float64) (x, y float64) {
	x = r * math.Cos(theta)
	y = r * math.Sin(theta)
	return
}

// Function using complex numbers
func complexMagnitude(c complex128) float64 {
	return cmplx.Abs(c)
}

// Function with deferred floating-point operation
func deferredDivision(a, b float64) (result float64) {
	defer func() {
		if b == 0 {
			result = math.NaN()
		}
	}()
	result = a / b
	return
}

// Function using floating-point recursion
func approximateSqrt(x, guess float64) float64 {
	if math.Abs(guess*guess-x) < Epsilon {
		return guess
	}
	return approximateSqrt(x, (guess+x/guess)/2)
}

// Function demonstrating floating-point precision issues
func precisionDemo() {
	a := 0.1
	b := 0.2
	c := 0.3
	fmt.Printf("a + b == c? %v\n", math.Abs((a+b)-c) < Epsilon)
}

// Function using floating-point maps
func trigonometricMap() map[string]float64 {
	return map[string]float64{
		"sin(π/4)": math.Sin(Pi / 4),
		"cos(π/4)": math.Cos(Pi / 4),
		"tan(π/4)": math.Tan(Pi / 4),
	}
}

// Function using floating-point channels
func generateSineWave(freq, amplitude float64, samples int, out chan<- float64) {
	for i := 0; i < samples; i++ {
		out <- amplitude * math.Sin(2*Pi*freq*float64(i)/float64(samples))
	}
	close(out)
}

// Function demonstrating switch with floating-point values
func classifyFloat(x float64) string {
	switch {
	case math.IsNaN(x):
		return "NaN"
	case math.IsInf(x, 1):
		return "+Inf"
	case math.IsInf(x, -1):
		return "-Inf"
	case x == 0:
		return "Zero"
	case x > 0:
		return "Positive"
	default:
		return "Negative"
	}
}

// Function demonstrating labeled break with floating-point loop
func findFirstAboveThreshold(threshold float64, values []float64) (float64, bool) {
	for _, v := range values {
		if v > threshold {
			return v, true
		}
	}
	return 0, false
}

// Function demonstrating select with floating-point channels
func selectFromChannels() {
	ch1 := make(chan float64)
	ch2 := make(chan float64)

	go func() {
		ch1 <- math.Pi
		close(ch1)
	}()
	go func() {
		ch2 <- math.E
		close(ch2)
	}()

	select {
	case v := <-ch1:
		fmt.Printf("Received from ch1: %.5f\n", v)
	case v := <-ch2:
		fmt.Printf("Received from ch2: %.5f\n", v)
	}
}

// Function demonstrating use of `defer`, `recover`, and floating-point panic
func safeDivision(a, b float64) (result float64) {
	defer func() {
		if r := recover(); r != nil {
			fmt.Println("Recovered from panic:", r)
			result = math.NaN()
		}
	}()
	if b == 0 {
		panic("division by zero")
	}
	return a / b
}

func main() {
	// Demonstrate variadic function
	fmt.Println("Sum of floats:", sumFloats(1.1, 2.2, 3.3))

	// Demonstrate multiple return values
	x, y := polarToCartesian(1, Pi/4)
	fmt.Printf("Polar to Cartesian: (x, y) = (%.2f, %.2f)\n", x, y)

	// Demonstrate complex number function
	c := complex(3, 4)
	fmt.Printf("Magnitude of complex number: %.2f\n", complexMagnitude(c))

	// Demonstrate deferred division
	fmt.Printf("Deferred division: %.2f\n", deferredDivision(10, 0))

	// Demonstrate recursive square root approximation
	fmt.Printf("Approximate sqrt(2): %.5f\n", approximateSqrt(2, 1))

	// Demonstrate floating-point precision issues
	precisionDemo()

	// Demonstrate floating-point map
	trigMap := trigonometricMap()
	for k, v := range trigMap {
		fmt.Printf("%s = %.5f\n", k, v)
	}

	// Demonstrate floating-point channels
	sineWave := make(chan float64)
	go generateSineWave(1, 1, 10, sineWave)
	fmt.Println("Sine wave samples:")
	for sample := range sineWave {
		fmt.Printf("%.5f ", sample)
	}
	fmt.Println()

	// Demonstrate switch with floating-point values
	fmt.Println("Classify float:", classifyFloat(math.NaN()))

	// Demonstrate labeled break with floating-point loop
	values := []float64{0.1, 0.2, 0.3, 0.4, 0.5}
	if v, found := findFirstAboveThreshold(0.35, values); found {
		fmt.Printf("First value above threshold: %.2f\n", v)
	} else {
		fmt.Println("No value above threshold found")
	}

	// Demonstrate select with floating-point channels
	selectFromChannels()

	// Demonstrate safe division with panic and recover
	fmt.Printf("Safe division: %.2f\n", safeDivision(10, 0))

	// Demonstrate runtime.GOARCH for floating-point architecture
	fmt.Printf("Floating-point architecture: %s\n", runtime.GOARCH)
}