//Code Block 1 (CB1)
public static int factorial(int result) {
  if(result <= 1) return 1;
  return result * factorial(result-1); }