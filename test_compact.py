"""Test script to exercise compact output formatting."""

big_list = list(range(50))
big_dict = {f"key_{i}": i * 10 for i in range(30)}
small_list = [1, 2, 3]
nested = {"a": {"b": {"c": [1, 2, 3]}}}


def main():
    x = big_list  # noqa: F841
    y = big_dict  # noqa: F841
    z = small_list  # noqa: F841
    n = nested  # noqa: F841
    print("breakpoint here")  # line 14


if __name__ == "__main__":
    main()
