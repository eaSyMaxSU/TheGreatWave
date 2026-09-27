CC ?= cc
CFLAGS ?= -std=c11 -O3 -flto -Wall -Wextra -Wshadow -fno-math-errno
LDFLAGS ?= -flto
LIBS = -lm

SRC = src/buf.c src/width.c src/wave.c src/geom.c src/path.c src/native.c src/json5.c \
      src/emit.c src/format.c src/render.c
OBJ = $(SRC:src/%.c=build/%.o)

.PHONY: all test asan bench clean

all: build/tgw

build:
	mkdir -p build

build/%.o: src/%.c src/common.h tgw.h src/width_tables.h | build
	$(CC) $(CFLAGS) -I. -c $< -o $@

build/main.o: src/main.c src/common.h tgw.h | build
	$(CC) $(CFLAGS) -I. -c src/main.c -o build/main.o

build/tgw: $(OBJ) build/main.o
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $(OBJ) build/main.o $(LIBS)

build/check: $(OBJ) tests/check.c | build
	$(CC) $(CFLAGS) -I. -o $@ tests/check.c $(OBJ) $(LIBS)

test: build/tgw build/check
	./build/check

asan:
	$(MAKE) clean
	$(MAKE) test CFLAGS="-std=c11 -O1 -g -Wall -Wextra -fsanitize=address -fno-omit-frame-pointer" \
		LDFLAGS="-fsanitize=address"

build/bench: $(OBJ) bench/long_wave.c | build
	$(CC) $(CFLAGS) -I. -o $@ bench/long_wave.c $(OBJ) $(LIBS)

bench: build/bench
	./build/bench

clean:
	rm -rf build
