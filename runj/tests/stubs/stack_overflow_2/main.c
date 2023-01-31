void fun(int x)
{
    fun(x + 1);
}
  
int main()
{
   fun(0);
}