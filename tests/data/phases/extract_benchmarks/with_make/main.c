#include "point.h"

int main() {
    Point p1 = {1, 2};
    Point p2 = {3, 4};

    print_point(p1);
    print_point(p2);

    Point sum = add_points(p1, p2);
    print_point(sum);

    return 0;
}
