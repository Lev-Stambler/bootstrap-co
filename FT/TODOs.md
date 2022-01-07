- [ ] Think about how to do storage. The current solution still lets people spam storage. I think the solution probably has to do something with in connection with the internal balance macro... internal balance may even have to keep track of a map of account id -> # tokens stored (account for that in the balance)
- [ ] Internal balance should remove an account when balance hits 0
- [ ] Almost worth it for internal bal to just have max # tokens per user or smthng,
or maybe not, maybe j track # of tokens and calc storage from there?
on account close --> delete entries in internal balances