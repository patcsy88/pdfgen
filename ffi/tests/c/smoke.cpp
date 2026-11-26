// Compiles the C smoke program as C++17. A C++ compiler mangles any function that pdfgen.h fails to wrap
// in extern "C", so this also fails to link if the header's linkage is wrong.
#define SMOKE_NO_MAIN
#include "smoke.c"

int main(int argc, char **argv) { return smoke_main(argc, argv); }
