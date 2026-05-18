function separateDigits(nums: number[]): number[] {
  const ans: number[] = [];
  for (const num of nums.toReversed()) {
    let n = num;
    while (n > 0) {
      ans.push(n % 10);
      n = Math.floor(n / 10);
    }
  }
  return ans.reverse();
}

// 测试用例
const testCases = [
  {
    nums: [13, 25, 83, 77],
    expected: [1, 3, 2, 5, 8, 3, 7, 7],
  },
];

testCases.forEach(({ nums, expected }, index) => {
  const result = separateDigits(nums);
  console.log(
    `Test case ${index + 1}:`,
    `result:`,
    result,
    String(result) === String(expected) ? "✅" : "❌",
  );
});
