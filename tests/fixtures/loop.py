"""Simple loop for integration testing with debugpy."""

def main():
    total = 0
    for i in range(5):
        total += i  # line 6: breakpoint target
    print(f"total = {total}")

if __name__ == "__main__":
    main()
