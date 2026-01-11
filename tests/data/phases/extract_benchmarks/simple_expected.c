struct Point { int x; int y; };

typedef struct Point Point;

static int helper(Point p) {
  return p.x + p.y;
}