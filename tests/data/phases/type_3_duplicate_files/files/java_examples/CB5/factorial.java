//Code Block 5 (CB5)
public int factorial(int result) {
    if(result == 0) {
        return 1;
    } else {
        return result * factorial(result-1); } }