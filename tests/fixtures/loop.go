// Simple loop for integration testing with delve.
package main

import "fmt"

func main() {
	total := 0
	for i := 0; i < 5; i++ {
		total += i // line 9: breakpoint target
	}
	fmt.Printf("total = %d\n", total)
}
