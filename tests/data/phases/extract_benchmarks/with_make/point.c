#include "point.h"

void print_point(Point p) {
    add_points(p, p);
 }

Point add_points(Point a, Point b) {
    Point result = { a.x + b.x, a.y + b.y };
    return result;
}
