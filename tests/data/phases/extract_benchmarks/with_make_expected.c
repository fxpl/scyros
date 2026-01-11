struct {
    int x;
    int y;
};

typedef struct {
    int x;
    int y;
} Point;

Point add_points(Point a, Point b) {
    Point result = { a.x + b.x, a.y + b.y };
    return result;
}

void print_point(Point p) {
    add_points(p, p);
 }

int main() {
    Point p1 = {1, 2};
    Point p2 = {3, 4};

    print_point(p1);
    print_point(p2);

    Point sum = add_points(p1, p2);
    print_point(sum);

    return 0;
}