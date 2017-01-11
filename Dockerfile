FROM jimmycuadra/rust

ADD . /my-source

RUN cd /my-source && cargo build -v --release

CMD ["/my-source/target/release/6EQUJ5"]
